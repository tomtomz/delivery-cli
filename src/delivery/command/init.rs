// Takes in fully parsed and defaulted init clap args,
// executes init codeflow, handles user actionable errors, as well as UI output.
//
// Returns an integer exit code, handling all errors it knows how to
// and panicing on unexpected errors.

use cli::init::InitClapOptions;

// TODO delete
use cli::load_config;
use config::Config;

// TODO delete more
use std::path::{Path, PathBuf};
use errors::{DeliveryError, Kind};

use project;
use git;
use utils;
use utils::say::sayln;

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

/// Handles the build-cookbook generation
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
        Some(generator_str) => {
            let gen_path = Path::new(&generator_str);
            let mut generator_path = cache_path.clone();
            generator_path.push(gen_path.file_stem().unwrap());
            project::custom_build_cookbook_generator(&gen_path, &cache_path).unwrap();
            project::chef_generate_build_cookbook_from_generator(&generator_path, &project_path).unwrap();
            let config_path = project_path.join(".delivery/config.json");
            if !(config_path.exists()) {
                sayln("red", "You used a custom build cookbook generator, but .delivery/config.json was not created.");
                sayln("red", "Please update your generator to create a valid .delivery/config.json or pass in a custom config.");
                return None;
            }
            return Some(true)
        },
        None => {
            if project::project_path().join(".delivery/build-cookbook").exists() {
                sayln("red", ".delivery/build-cookbook folder already exists, skipping build cookbook generation.");
                return Some(false)
            } else {
                let gen = project::create_default_build_cookbook();
                git::git_push_master().unwrap();
                sayln("green", &format!("Build-cookbook generated: {:#?}", gen));
                return Some(false)
            }
        }
    };
}

pub fn init(config: Config, no_open: &bool, skip_build_cookbook: &bool,
            local: &bool, scp: Option<project::SourceCodeProvider>) -> i32 {
    let project_path = project::root_dir(&utils::cwd()).unwrap();
    project::create_dot_delivery();
    project::create_on_server(&config, scp.clone(), local).unwrap();

    // If non-custom generator used, then build cookbook is already merged to master.
    let mut custom_build_cookbook_generated = false;
    if !(*skip_build_cookbook) {
        custom_build_cookbook_generated = match generate_build_cookbook(config.generator().ok()) {
            Some(boolean) => boolean,
            None => return 1
        }
    }

    let custom_config_passed = project::generate_custom_delivery_config(config.config_json().ok()).unwrap();

    // If we need a branch for either the custom build cookbook or custom config, create it.
    // If nothing custom was requested, then `chef generate build-cookbook` will handle the commits for us.
    if custom_build_cookbook_generated || custom_config_passed {
        project::create_feature_branch_if_missing(&project_path).unwrap();

        if custom_build_cookbook_generated {
            project::add_commit_build_cookbook(&custom_config_passed).unwrap();
        }
        // Only trigger review if there were any custom commits to review.
        project::trigger_review(config, scp, &no_open, &local).unwrap();
    } else {
        if let Some(project_type) = scp {
            if project_type.kind == project::Type::Github {
                let _ = project::check_github_remote(project_type).unwrap();
            }
        };

        sayln("white", "\nBuild cookbook generated and pushed to master in delivery.");
        // TODO: Once we want people to use the local command, uncomment this.
        //sayln("white", "As a first step, try running:\n");
        //sayln("white", "delivery local lint");
    }
    return 0
}

