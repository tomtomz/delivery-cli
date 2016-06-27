// Takes in fully parsed and defaulted init clap args,
// executes init codeflow, handles user actionable errors, as well as UI output.
//
// Returns an integer exit code, handling all errors it knows how to
// and panicing on unexpected errors.

use cli::init::InitClapOptions;
use delivery_config::DeliveryConfig;
use cli;

// TODO delete
use cli::load_config;
use config::Config;

// TODO delete more
use std::path::{Path, PathBuf};
use errors::{DeliveryError, Kind};

use project;
use git;
use utils;
use utils::say::{sayln, say};

/// Initialize a Delivery project
///
/// This method will init a Delivery project doing the following:
/// * Create the project in Delivery. (It knows how to link the project to a
///   Github or Bitbucket SCP)
/// * Add the `delivery` remote (Only Delivery & Bitbucket projects)
/// * Push local content to Delivery (Only Delivery & Bitbucket projects)
/// * Create a Pipeline
/// * Create a feature branch called `add-delivery-config` to:
///     * Create a build-cookbook
///     * Create the `.delivery/config.json`
/// * Finally submit a cli::review (Only for Delivery & Bitbucket projects)
///
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

    return init(config, &init_opts.no_open, &init_opts.skip_build_cookbook, &init_opts.local, scp);
}
    
pub fn init(config: Config, no_open: &bool, skip_build_cookbook: &bool,
            local: &bool, scp: Option<project::SourceCodeProvider>) -> i32 {
    let project_path = project::project_path();
    project::create_dot_delivery();
    project::create_on_server(&config, scp.clone(), local).unwrap();

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

        // Only trigger review if there were any custom commits to review.
        if !(local) {
            trigger_review(config, scp, &no_open).unwrap()
        }
    } else {
        if let Some(project_type) = scp {
            if project_type.kind == project::Type::Github {
                let _ = check_github_remote(project_type);
            }
        };

        sayln("white", "\nBuild cookbook generated and pushed to master in delivery.");
        // TODO: Once we want people to use the local command, uncomment this.
        //sayln("white", "As a first step, try running:\n");
        //sayln("white", "delivery local lint");
    }
    return 0
}

// Handles the build-cookbook generation
///
/// This method could receive a custom generator, if it is not provided,
/// we use the default build-cookbook generator from the ChefDK.
///
/// Returns true if a CUSTOM build cookbook was generated, else false if something went wrong.
fn generate_build_cookbook(generator: Option<String>) -> Option<bool> {
    sayln("white", "Generating build cookbook skeleton");
    let cache_path = project::generator_cache_path().unwrap();
    let project_path = project::root_dir(&utils::cwd()).unwrap();
    match generator {
        // Custom build cookbook
        Some(generator_str) => {
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
            
            let command = project::chef_generate_build_cookbook_from_generator(&generator_path, &cache_path, &project_path);
            sayln("green", &format!("Build-cookbook generated: {:#?}", command));

            let config_path = project_path.join(".delivery/config.json");
            if !(config_path.exists()) {
                sayln("red", "You used a custom build cookbook generator, but .delivery/config.json was not created.");
                sayln("red", "Please update your generator to create a valid .delivery/config.json or pass in a custom config.");
                return None;
            }
            return Some(true)
        },
        // Default build cookbook
        None => {
            if project::project_path().join(".delivery/build-cookbook").exists() {
                sayln("red", ".delivery/build-cookbook folder already exists, skipping build cookbook generation.");
                return Some(false)
            } else {
                let command = project::create_default_build_cookbook();
                git::git_push_master().unwrap();
                sayln("green", &format!("Build-cookbook generated: {:#?}", command));
                return Some(false)
            }
        }
    };
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
        DeliveryConfig::git_add_commit_config(&json_path);
        sayln("green", "done");

        return true
    } else {
        return false
    }
}

/// Triggers a delvery review
fn trigger_review(config: Config, scp: Option<project::SourceCodeProvider>,
                  no_open: &bool) -> Result<(), DeliveryError> {    
    let pipeline = config.pipeline().unwrap();
    match scp {
        Some(s) => {
            match s.kind {
                project::Type::Bitbucket => {
                    try!(cli::review(&pipeline, &false, no_open, &false));
                },
                project::Type::Github => {
                    // For now, delivery review doesn't works for Github projects
                    // TODO: Make it work in github
                    sayln("green", "\nYour project is now set up with changes in the add-delivery-config branch!");
                    sayln("green", "To finalize your project, you must submit and accept a Pull Request in github.");

                    check_github_remote(s);

                    sayln("green", "Push your project to github by running:\n");
                    sayln("green", "git push origin add-delivery-config\n");
                    sayln("green", "Then log into github via your browser, make a Pull Request, then comment `@delivery approve`.");
                }
            }
        },
        None => try!(cli::review(&pipeline, &false, no_open, &false))
    }
    Ok(())
}

// Check to see if the origin remote is set up, and if not, output something useful.
fn check_github_remote(s: project::SourceCodeProvider) -> bool {
    let git_remote_result = git::git_command(&["remote"], &project::project_path());
    match git_remote_result {
        Ok(git_result) => {
            if !(git_result.stdout.contains("origin")) {
                sayln("green", "First, you must add your remote.");
                sayln("green", "Run this if you want to use ssh:\n");
                sayln("green", &format!("git remote add origin git@github.com:{}/{}.git\n", s.organization, s.repo_name));
                sayln("green", "Or this for https:\n");
                sayln("green", &format!("git remote add origin https://github.com/{}/{}.git\n", s.organization, s.repo_name));
            }
            true
        },
        Err(_) => false
    }
}

