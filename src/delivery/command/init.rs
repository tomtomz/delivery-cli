// Takes in fully parsed and defaulted init clap args,
// executes init codeflow, handles user actionable errors, as well as UI output.
//
// Returns an integer exit code, handling all errors it knows how to
// and panicing on unexpected errors.

use cli::init::InitClapOptions;
use delivery_config::DeliveryConfig;
use cli::load_config;
use config::Config;
use std::path::{Path, PathBuf};
use project;
use git;
use utils;
use utils::say::{sayln, say};
use http::APIClient;
use errors::{Kind, DeliveryError};
use types::{DeliveryResult, ExitCode};

pub fn run(init_opts: InitClapOptions) -> DeliveryResult<ExitCode> {
    sayln("green", "Chef Delivery");
    let mut config = try!(load_config(&utils::cwd()));
    let final_proj = try!(project::project_or_from_cwd(init_opts.project));
    config = config.set_user(init_opts.user)
        .set_server(init_opts.server)
        .set_enterprise(init_opts.ent)
        .set_organization(init_opts.org)
        .set_project(&final_proj)
        .set_pipeline(init_opts.pipeline)
        .set_generator(init_opts.generator)
        .set_config_json(init_opts.config_json);
    let branch = try!(config.pipeline());

    if !init_opts.github_org_name.is_empty()
        && !init_opts.bitbucket_project_key.is_empty() {
        sayln("red", "\nPlease specify just one Source Code Provider: \
              delivery (default), github or bitbucket.");
        return Ok(1)
    }

    let scp = if !init_opts.github_org_name.is_empty() {
        Some(
            try!(project::SourceCodeProvider::new("github", &init_opts.repo_name,
                                                  &init_opts.github_org_name, &branch,
                                                  init_opts.no_v_ssl))
        )
    } else if !init_opts.bitbucket_project_key.is_empty() {
        Some(
            try!(project::SourceCodeProvider::new("bitbucket", &init_opts.repo_name,
                                                  &init_opts.bitbucket_project_key,
                                                  &branch, true))
        )
    } else {
        None
    };

    // Initalize the repo.
    let project_path = project::project_path();
    project::create_dot_delivery();

    if !init_opts.local {
        try!(create_on_server(&config, scp.clone()))
    }

    // Generate build cookbook, either custom or default.
    //let mut custom_build_cookbook_generated = false;
    let custom_build_cookbook_generated = if !init_opts.skip_build_cookbook {
        match generate_build_cookbook(&config) {
            Ok(boolean) => boolean,
            // Custom error handling for missing config file.
            // Pass back 1 to avoid additional default error handling.
            Err(DeliveryError{ kind: Kind::MissingConfigFile, .. }) => {
                sayln("red", "\nYou used a custom build cookbook generator, but \
                      .delivery/config.json was not created.");
                sayln("red", "Please update your generator to create a valid \
                      .delivery/config.json or pass in a custom config.");
                return Ok(1)
            },
            // Unexpected error, pass back.
            Err(e) => return Err(e)
        }
    } else {
        false
    };

    let custom_config_passed = try!(
        generate_delivery_config(config.config_json().ok())
    );

    // If nothing custom was requested, then `chef generate build-cookbook`
    // will handle the commits for us. If we need a branch for either the
    // custom build cookbook or custom config, create it.
    if custom_build_cookbook_generated || custom_config_passed {
        sayln("blue", "Committing unmerged Delivery content and submitting for review...");
        if !try!(project::create_feature_branch_if_missing(&project_path)) {
            sayln("white", "  Skipping: A branch named 'add-delivery-config' already exists, \
                            switching to it.");
        } else {
            sayln("green", "  Feature branch named 'add-delivery-config' created.")
        }

        if custom_build_cookbook_generated {
            try!(project::add_commit_build_cookbook(&custom_config_passed));
            sayln("green", "  Custom build cookbook committed to feature branch.")
        }

        // project::add_commit_build_cookbook will commit the custom config for us,
        // so if a custom build cookbook was passed, the delivery config was already commited.
        if custom_config_passed && !custom_build_cookbook_generated {
            try!(DeliveryConfig::git_add_commit_config(&project_path));
            sayln("green", "  Custom delivery config committed to feature branch.")
        }

        // Trigger review if there were any custom commits to review.
        if !init_opts.local {
            sayln("blue", "Submitting feature branch 'add-delivery-config' for review...");
            try!(trigger_review(config, scp, &init_opts.no_open));
            sayln("green", "");
        } else {
            sayln("white", "  You passed --local, skipping review submission.");
            sayln("green", "\nYour new project is ready!")
        }
    } else {
        if let Some(project_type) = scp {
            if project_type.kind == project::Type::Github {
                if try!(project::missing_github_remote()) {
                    setup_github_remote_msg(&project_type)
                }
            }
        };

        sayln("green", "\nYour new project is ready!");
        // TODO: Once we want people to use the local command, uncomment this.
        //sayln("white", "As a first step, try running:\n");
        //sayln("white", "delivery local lint");
    }
    Ok(0)
}

// Create a Delivery Project
//
// This method will create a Delivery Project depending on the SCP that we specify,
// either a Github, Bitbucket or Delivery (default). It also creates a pipeline,
// adds the `delivery` remote and push the content of the local repo to the Server.
fn create_on_server(config: &Config,
                    scp: Option<project::SourceCodeProvider>) -> DeliveryResult<()> {
    let client = try!(APIClient::from_config(config));
    let git_url = try!(config.delivery_git_ssh_url());
    let org = try!(config.organization());
    let proj = try!(config.project());
    let pipe = try!(config.pipeline());

    match scp {
        // If the user requested a custom scp
        Some(scp_config) => {
            // TODO: actually handle this error
            try!(scp_config.verify_server_config(&client));

            say("white", "Creating ");
            match scp_config.kind {
                project::Type::Bitbucket => {
                    say("magenta", "bitbucket");
                    say("white", " project: ");
                    say("magenta", &format!("{} ", proj));
                    try!(create_bitbucket_project(org, proj, pipe, git_url,
                                                  client, scp_config))
                },
                project::Type::Github => {
                    say("magenta", "github");
                    say("white", " project: ");
                    say("magenta", &format!("{} ", proj));
                    // TODO: http code still outputs, move output here.
                    try!(client.create_github_project(&org, &proj, &scp_config.repo_name,
                                                 &scp_config.organization, &scp_config.branch,
                                                 scp_config.verify_ssl));
                }
            }
        },
        // If the user isn't using an scp, just delivery itself.
        None => {
            // Create delivery project on server unless it already exists.
            sayln("blue", "Creating delivery project...");
            if try!(project::create_delivery_project(&client, &org, &proj)) {
                sayln("green", &format!("  Delivery project named {} created.", proj));
            } else {
                sayln("white",
                      &format!("  Skipping: Delivery project named {} already exists.", proj));
            }

            // Setup delivery remote
            sayln("blue", "Creating delivery git remote...");
            if try!(project::create_delivery_remote_if_missing(&git_url)) {
                sayln("green", &format!("  Remote 'delivery' added as {}.", git_url))
            } else {
                sayln("white",
                      "  Skipping: Remote named 'delivery' already exists and is correct.")
            }

            // Push content to master if no upstream commits.
            sayln("blue", "Pushing initial git history...");
            if !try!(project::push_project_content_to_delivery(&pipe)) {
                sayln("white", &format!("  Skipping: Found commits on remote for pipeline {}, \
                                         not pushing local commits.", pipe))
            } else {
                sayln("green", &format!("  No git history found for pipeline {}, \
                                         pushing local commits from branch {}.", pipe, pipe))
            }

            // Create delivery pipeline unless it already exists.
            sayln("blue", "Creating pipline on delivery server...");
            if try!(project::create_delivery_pipeline(&client, &org, &proj, &pipe)) {
                sayln("green", &format!("  Created Delivery pipeline {} for project {}.",
                                      pipe, proj))
            } else {
                sayln("white", &format!("  Skipping: Delivery pipeline \
                                         named {} already exists for project {}.", pipe, proj))
            }
        }
    }
    Ok(())
}

fn create_bitbucket_project(org: String, proj: String, pipeline: String,
                            git_url: String, client: APIClient,
                            scp_config: project::SourceCodeProvider) -> DeliveryResult<()> {
    // TODO: http code still outputs, move output here.
    try!(client.create_bitbucket_project(&org, &proj, &scp_config.repo_name,
                                         &scp_config.organization, &scp_config.branch));
    // Setup delivery remote
    if try!(project::create_delivery_remote_if_missing(&git_url)) {
        sayln("green", &format!("  Remote 'delivery' added as {}", git_url))
    } else {
        sayln("white",
              "  Skipping: Remote named 'delivery' already exists and is correct.")
    }

    // Push content to master if no upstream commits
    say("white", "Checking for content on the git remote ");
    say("magenta", "delivery: ");
    if !try!(project::push_project_content_to_delivery(&pipeline)) {
        sayln("red", "Found commits upstream, not pushing local commits");
    };
    Ok(())
}

// Handles the build-cookbook generation
//
// Use the provided custom generator, if it is not provided we use the default
// build-cookbook generator from the ChefDK.
//
// Returns true if a CUSTOM build cookbook was generated, else it returns false.
fn generate_build_cookbook(config: &Config) -> DeliveryResult<bool> {
    let cache_path = try!(project::generator_cache_path());
    let project_path = project::project_path();
    match config.generator().ok() {
        Some(generator_str) => {
            sayln("blue", "Generating custom build cookbook...");
            generate_custom_build_cookbook(generator_str, cache_path, project_path)
        },
        // Default build cookbook
        None => {
            sayln("blue", "Generating default build cookbook...");
            if project::project_path().join(".delivery/build-cookbook").exists() {
                sayln("white", "  Skipping: build cookbook already exists at \
                                .delivery/build-cookbook.");
                Ok(false)
            } else {
                try!(project::create_default_build_cookbook());
                sayln("green", "  Build cookbook generated at .delivery/build-cookbook.");
                let pipeline = try!(config.pipeline());
                try!(git::git_push(&pipeline));
                sayln("green",
                      &format!("  Build cookbook commited to git and pushed to pipeline named {}.", pipeline));
                Ok(false)
            }
        }
    }
}

fn generate_custom_build_cookbook(generator_str: String,
                                  cache_path: PathBuf,
                                  project_path: PathBuf) -> DeliveryResult<bool> {
    let gen_path = Path::new(&generator_str);
    let mut generator_path = cache_path.clone();
    generator_path.push(gen_path.file_stem().unwrap());
    match try!(project::download_or_mv_custom_build_cookbook_generator(&gen_path, &cache_path)) {
        project::CustomCookbookSource::Disk => {
            sayln("green", "  Copying custom build cookbook generator to the cache.")
        },
        project::CustomCookbookSource::Cached => {
            sayln("white", "  Skipping: Using cached copy of custom build cookbook generator.")
        },
        project::CustomCookbookSource::Git => {
            sayln("green", &format!("  Downloading build-cookbook generator from {}", generator_str))
        }
    }
    
    try!(project::chef_generate_build_cookbook_from_generator(&generator_path,
                                                              &project_path));
    sayln("green", "  Custom build cookbook generated at .delivery/build-cookbook.");

    let config_path = project_path.join(".delivery/config.json");
    if !config_path.exists() {
        return Err(DeliveryError{ kind: Kind::MissingConfigFile, detail: None });
    }
    return Ok(true)
}

fn generate_delivery_config(config_json: Option<String>) -> DeliveryResult<bool> {
    if let Some(json) = config_json {
        sayln("blue", "Coping custom Delivery config...");
        let proj_path = project::project_path();
        let json_path = PathBuf::from(&json);

        // Create config
        match try!(DeliveryConfig::copy_config_file(&json_path, &proj_path)) {
            Some(_) => {
                sayln("green", &format!("  Custom Delivery config copied \
                                         from {} to .delivery/config.json.", &json));
                Ok(true)
            },
            None => {
                sayln("white", &format!("  Skipped: Content of custom config passed from {} exactly matches existing .delivery/config.json.", &json));
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

// Triggers a delvery review.
fn trigger_review(config: Config, scp: Option<project::SourceCodeProvider>,
                  no_open: &bool) -> DeliveryResult<()> {    
    let pipeline = try!(config.pipeline());
    let head = try!(git::get_head());
    match scp {
        Some(s) => {
            match s.kind {
                project::Type::Bitbucket => {
                    let review = try!(project::review(&pipeline, &head));
                    try!(project::handle_review_result(&review, no_open));
                    sayln("green", "  Review submitted to Delivery \
                                    with Bitbucket intergration enabled.");

                },
                project::Type::Github => {
                    // For now, delivery review doesn't works for Github projects
                    // TODO: Make it work in github
                    sayln("green", "\nYour project is now set up with changes in the \
                          add-delivery-config branch!");
                    sayln("green", "To finalize your project, you must submit and \
                          accept a Pull Request in github.");

                    if try!(project::missing_github_remote()) {
                        setup_github_remote_msg(&s)
                    }

                    sayln("green", "Push your project to github by running:\n");
                    sayln("green", "git push origin add-delivery-config\n");
                    sayln("green", "Then log into github via your browser, make a \
                          Pull Request, then comment `@delivery approve`.");
                }
            }
        },
        None => {
            let review = try!(project::review(&pipeline, &head));
            try!(project::handle_review_result(&review, no_open));
            sayln("green", "  Review submitted to Delivery.");
            // TODO
            sayln("green", "\nYour new project is ready!");
        }
    }
    Ok(())
}

fn setup_github_remote_msg(s: &project::SourceCodeProvider) -> () {
    sayln("green", "First, you must add your remote.");
    sayln("green", "Run this if you want to use ssh:\n");
    sayln("green", &format!(
            "git remote add origin git@github.com:{}/{}.git\n",
            s.organization, s.repo_name));
    sayln("green", "Or this for https:\n");
    sayln("green", &format!(
            "git remote add origin https://github.com/{}/{}.git\n",
            s.organization, s.repo_name));
}
