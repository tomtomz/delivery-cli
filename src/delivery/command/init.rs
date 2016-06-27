// Takes in fully parsed and defaulted init clap args,
// executes init codeflow, handles user actionable errors, as well as UI output.
//
// Returns an integer exit code, handling all errors it knows how to
// and panicing on unexpected errors.

use cli::init::InitClapOptions;
use delivery_config::DeliveryConfig;
use cli;
use cli::load_config;
use config::Config;
use std::path::{Path, PathBuf};
use project;
use git;
use utils;
use utils::say::{sayln, say};
use http::APIClient;

pub fn run(init_opts: InitClapOptions) -> i32 {
    sayln("green", "Chef Delivery");
    let mut config = load_config(&utils::cwd()).unwrap();
    let final_proj = project::project_or_from_cwd(init_opts.project).unwrap();
    config = config.set_user(init_opts.user)
        .set_server(init_opts.server)
        .set_enterprise(init_opts.ent)
        .set_organization(init_opts.org)
        .set_project(&final_proj)
        .set_pipeline(init_opts.pipeline)
        .set_generator(init_opts.generator)
        .set_config_json(init_opts.config_json);
    let branch = config.pipeline().unwrap();

    if !init_opts.github_org_name.is_empty() && !init_opts.bitbucket_project_key.is_empty() {
        sayln("red", "Please specify just one Source Code Provider: delivery(default), github or bitbucket.");
        return 1;
    }

    let mut scp: Option<project::SourceCodeProvider> = None;
    if !init_opts.github_org_name.is_empty() {
        scp = Some(
            project::SourceCodeProvider::new("github", &init_opts.repo_name,
                                             &init_opts.github_org_name, &branch,
                                             init_opts.no_v_ssl).unwrap()
        );
    } else if !init_opts.bitbucket_project_key.is_empty() {
        scp = Some(
            project::SourceCodeProvider::new("bitbucket", &init_opts.repo_name,
                                             &init_opts.bitbucket_project_key,
                                             &branch, true).unwrap()
        );
    }

    // Returns integer exit status.
    return init(config, &init_opts.no_open, &init_opts.skip_build_cookbook, &init_opts.local, scp);
}

// Once everything is validated, actually execute init.
// Handles logic flow, output, and error handling.
//
// Initialize a Delivery project
//
// This method will init a Delivery project doing the following:
// * Create the project in Delivery. (It knows how to link the project to a
//   Github or Bitbucket SCP)
// * Add the `delivery` remote (Only Delivery & Bitbucket projects)
// * Push local content to Delivery (Only Delivery & Bitbucket projects)
// * Create a Pipeline
// * Create a feature branch called `add-delivery-config` to:
//     * Create a build-cookbook
//     * Create the `.delivery/config.json`
// * Finally submit a cli::review (Only for Delivery & Bitbucket projects)
//
fn init(config: Config, no_open: &bool, skip_build_cookbook: &bool,
            local: &bool, scp: Option<project::SourceCodeProvider>) -> i32 {
    let project_path = project::project_path();
    project::create_dot_delivery();

    if !(local) {
        create_on_server(&config, scp.clone())
    }

    // Generate build cookbook, either custom or default.
    let mut custom_build_cookbook_generated = false;
    if !(*skip_build_cookbook) {
        custom_build_cookbook_generated = match generate_build_cookbook(config.generator().ok()) {
            Some(boolean) => boolean,
            None => return 1
        }
    }

    let custom_config_passed = generate_delivery_config(config.config_json().ok());

    // If we need a branch for either the custom build cookbook or custom config, create it.
    // If nothing custom was requested, then `chef generate build-cookbook` will handle the commits for us.
    if custom_build_cookbook_generated || custom_config_passed {
        say("white", "Creating and checking out ");
        say("yellow", "add-delivery-config");
        say("white", " feature branch: ");
        if project::create_feature_branch_if_missing(&project_path) {
            say("white", "A branch named 'add-delivery-config' already exists, switching to it.\n");
        }

        if custom_build_cookbook_generated {
            say("white", "Adding and commiting build-cookbook: ");
            project::add_commit_build_cookbook(&custom_config_passed);
            sayln("green", "done");
        }

        // Trigger review if there were any custom commits to review.
        if !(local) {
            trigger_review(config, scp, &no_open)
        }
    } else {
        if let Some(project_type) = scp {
            if project_type.kind == project::Type::Github {
                if !(project::check_github_remote()) {
                    setup_github_remote_msg(&project_type)
                }
            }
        };

        sayln("white", "\nBuild cookbook generated and pushed to master in delivery.");
        // TODO: Once we want people to use the local command, uncomment this.
        //sayln("white", "As a first step, try running:\n");
        //sayln("white", "delivery local lint");
    }
    return 0
}

// Create a Delivery Project
//
// This method will create a Delivery Project depending on the SCP that we specify,
// either a Github, Bitbucket or Delivery (default). It also creates a pipeline,
// adds the `delivery` remote and push the content of the local repo to the Server.
fn create_on_server(config: &Config,
              scp: Option<project::SourceCodeProvider>) -> () {
    let client = APIClient::from_config(config).unwrap();
    match scp {
        // If the user requested a custom scp
        Some(scp_config) => {
            // TODO: actually handle this error
            scp_config.verify_server_config(&client).unwrap();

            let org = config.organization().unwrap();
            let proj = config.project().unwrap();
            let git_url = config.delivery_git_ssh_url().unwrap();

            say("white", "Creating ");
            match scp_config.kind {
                project::Type::Bitbucket => {
                    say("magenta", "bitbucket");
                    say("white", " project: ");
                    say("magenta", &format!("{} ", proj));
                    create_bitbucket_project(org, proj, git_url, client, scp_config)
                },
                project::Type::Github => {
                    say("magenta", "github");
                    say("white", " project: ");
                    say("magenta", &format!("{} ", proj));
                    // TODO: http code still outputs, move output here.
                    client.create_github_project(&org, &proj, &scp_config.repo_name,
                                                 &scp_config.organization, &scp_config.branch,
                                                 scp_config.verify_ssl).unwrap();
                }
            }
        },
        // If the user isn't using an scp, just delivery itself.
        None => {
            let org = config.organization().unwrap();
            let proj = config.project().unwrap();
            let pipe = config.pipeline().unwrap();

            // Create delivery project on server unless it already exists.
            if project::create_delivery_project(&client, &org, &proj) {
                say("white", "Created ");
                say("magenta", "delivery");
                say("white", " project: ");
                say("magenta", &format!("{} ", proj));
            } else {
                say("white", "Project ");
                say("magenta", &format!("{} ", proj));
                sayln("white", "already exists.")
            }

            // Push content to master if no upstream commits.
            say("white", "Checking for content on the git remote ");
            say("magenta", "delivery: ");
            if !(project::push_project_content_to_delivery()) {
                sayln("red", "Found commits upstream, not pushing local commits")
            }

            // Create delivery pipeline unless it already exists.
            if project::create_delivery_pipeline(&client, &org, &proj, &pipe) {
                say("white", "Created ");
                say("magenta", &format!("{}", pipe));
                say("white", " pipeline for project: ");
                say("magenta", &format!("{}: ", proj))
            } else {
                say("white", "Pipeline ");
                say("magenta", &format!("{} ", pipe));
                sayln("white", "already exists.")
            }
        }
    }
    ()
}

fn create_bitbucket_project(org: String, proj: String, git_url: String, client: APIClient, scp_config: project::SourceCodeProvider) -> () {
    // TODO: http code still outputs, move output here.
    client.create_bitbucket_project(&org, &proj, &scp_config.repo_name,
                                    &scp_config.organization, &scp_config.branch).unwrap();
    // Setup delivery remote
    if project::create_delivery_remote_if_missing(git_url) {
        sayln("white", "Remote 'delivery' added to git config!")
    } else {
        sayln("white", "Remote named 'delivery' already exists and is correct - not modifying")
    }

    // Push content to master if no upstream commits
    say("white", "Checking for content on the git remote ");
    say("magenta", "delivery: ");
    if !(project::push_project_content_to_delivery()) {
        sayln("red", "Found commits upstream, not pushing local commits");
    }
}

// Handles the build-cookbook generation
//
// This method could receive a custom generator, if it is not provided,
// we use the default build-cookbook generator from the ChefDK.
//
// Returns true if a CUSTOM build cookbook was generated, else false if something went wrong.
fn generate_build_cookbook(generator: Option<String>) -> Option<bool> {
    sayln("white", "Generating build cookbook skeleton");
    let cache_path = project::generator_cache_path().unwrap();
    let project_path = project::root_dir(&utils::cwd()).unwrap();
    match generator {
        Some(generator_str) => {
            generate_custom_build_cookbook(generator_str, cache_path, project_path)
        },
        // Default build cookbook
        None => {
            if project::project_path().join(".delivery/build-cookbook").exists() {
                sayln("red", ".delivery/build-cookbook folder already exists, skipping build cookbook generation.");
                return Some(false)
            } else {
                let command = project::create_default_build_cookbook();
                // TODO: move output up to init post --for bugfix.
                git::git_push_master().unwrap();
                sayln("green", &format!("Build-cookbook generated: {:#?}", command));
                return Some(false)
            }
        }
    }
}

fn generate_custom_build_cookbook(generator_str: String, cache_path: PathBuf, project_path: PathBuf) -> Option<bool> {
    let gen_path = Path::new(&generator_str);
    let mut generator_path = cache_path.clone();
    generator_path.push(gen_path.file_stem().unwrap());
    match project::download_or_mv_custom_build_cookbook_generator(&gen_path, &cache_path) {
        project::CustomCookbookSource::Disk => {
            say("white", "Copying custom build-cookbook generator to ");
            sayln("yellow", &format!("{:?}", &cache_path));
        },
        project::CustomCookbookSource::Cached => {
            sayln("yellow", &format!("Using cached copy of build-cookbook generator {:?}",
                                     &cache_path));
        },
        project::CustomCookbookSource::Git => {
            say("white", "Downloading build-cookbook generator from ");
            sayln("yellow", &format!("{:?}", &generator_str));
        }
    }
    
    let command = project::chef_generate_build_cookbook_from_generator(&generator_path, &project_path);
    sayln("green", &format!("Build-cookbook generated: {:#?}", command));

    let config_path = project_path.join(".delivery/config.json");
    if !(config_path.exists()) {
        sayln("red", "You used a custom build cookbook generator, but .delivery/config.json was not created.");
        sayln("red", "Please update your generator to create a valid .delivery/config.json or pass in a custom config.");
        return None;
    }
    return Some(true)
}
    
fn generate_delivery_config(config_json: Option<String>) -> bool {
    if let Some(json) = config_json {
        let proj_path = project::project_path();
        let json_path = PathBuf::from(json);

        // Create config
        let content = DeliveryConfig::copy_config_file(&json_path, &proj_path);

        // TODO: cleanup output
        say("white", "Copying configuration to ");
        sayln("yellow", &format!("{:?}", DeliveryConfig::config_file_path(&proj_path)));
        sayln("magenta", "New delivery configuration");
        sayln("magenta", "--------------------------");
        sayln("white", &content);

        // Commit to git
        say("white", "Git add and commit delivery config: ");
        DeliveryConfig::git_add_commit_config(&json_path).unwrap();
        sayln("green", "done");

        return true
    } else {
        return false
    }
}

// Triggers a delvery review.
fn trigger_review(config: Config, scp: Option<project::SourceCodeProvider>,
                  no_open: &bool) -> () {    
    let pipeline = config.pipeline().unwrap();
    match scp {
        Some(s) => {
            match s.kind {
                project::Type::Bitbucket => {
                    cli::review(&pipeline, &false, no_open, &false).unwrap();
                },
                project::Type::Github => {
                    // For now, delivery review doesn't works for Github projects
                    // TODO: Make it work in github
                    sayln("green", "\nYour project is now set up with changes in the add-delivery-config branch!");
                    sayln("green", "To finalize your project, you must submit and accept a Pull Request in github.");

                    if !(project::check_github_remote()) {
                        setup_github_remote_msg(&s)
                    }

                    sayln("green", "Push your project to github by running:\n");
                    sayln("green", "git push origin add-delivery-config\n");
                    sayln("green", "Then log into github via your browser, make a Pull Request, then comment `@delivery approve`.");
                }
            }
        },
        None => cli::review(&pipeline, &false, no_open, &false).unwrap()
    }
    ()
}

fn setup_github_remote_msg(s: &project::SourceCodeProvider) -> () {
    sayln("green", "First, you must add your remote.");
    sayln("green", "Run this if you want to use ssh:\n");
    sayln("green", &format!("git remote add origin git@github.com:{}/{}.git\n", s.organization, s.repo_name));
    sayln("green", "Or this for https:\n");
    sayln("green", &format!("git remote add origin https://github.com/{}/{}.git\n", s.organization, s.repo_name));
}
