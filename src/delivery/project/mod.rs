//
// Copyright:: Copyright (c) 2015 Chef Software, Inc.
// License:: Apache License, Version 2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

use utils::{self, walk_tree_for_path, mkdir_recursive};
use utils::path_ext::is_dir;
use errors::{DeliveryError, Kind};
use std::path::{Path, PathBuf};
use http::APIClient;
use git;
use std::process::{Output, Command};
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bitbucket,
    Github
}

#[derive(Debug, Clone)]
pub struct SourceCodeProvider {
    pub repo_name: String,
    pub organization: String,
    pub branch: String,
    pub verify_ssl: bool,
    pub kind: Type,
}

impl SourceCodeProvider {
    // Create a new `SourceCodeProvider`. Returns an error result if
    // required configuration values are missing. Expects to find
    // `scp`, `repository`, `scp-organization`, `branch`, and `ssl`.
    pub fn new(scp: &str, repo: &str, org: &str, branch: &str,
               no_ssl: bool) -> Result<SourceCodeProvider, DeliveryError> {
        let scp_kind = match scp {
            "github" => Type::Github,
            "bitbucket" => Type::Bitbucket,
            _ => return Err(DeliveryError{ kind: Kind::UnknownProjectType, detail:None })
        };
        if repo.to_string().is_empty()
            || org.to_string().is_empty()
            || branch.to_string().is_empty() {
            match scp_kind {
                Type::Github => return Err(
                    DeliveryError{
                        kind: Kind::OptionConstraint,
                        detail: Some(format!("Missing Github Source Code Provider attributes, specify: \
                                              repo-name, org-name and pipeline(default: master)"))
                    }
                ),
                Type::Bitbucket => return Err(
                    DeliveryError{
                        kind: Kind::OptionConstraint,
                        detail: Some(format!("Missing Bitbucket Source Code Provider attributes, specify: \
                                              repo-name, project-key and pipeline(default: master)"))
                    }
                ),
            }
        }
        Ok(SourceCodeProvider {
            kind: scp_kind,
            repo_name: repo.to_string(),
            organization: org.to_string(),
            branch: branch.to_string(),
            verify_ssl: !no_ssl,
        })
    }

    // Verify if the SCP is configured on the Delivery Server
    pub fn verify_server_config(&self, client: &APIClient) -> Result<(), DeliveryError> {
        match self.kind {
            Type::Github => {
                let scp_config = try!(client.get_github_server_config());
                if scp_config.is_empty() {
                    return Err(DeliveryError{ kind: Kind::NoGithubSCPConfig, detail: None })
                }
            },
            Type::Bitbucket => {
                let scp_config = try!(client.get_bitbucket_server_config());
                if scp_config.is_empty() {
                    return Err(DeliveryError{ kind: Kind::NoBitbucketSCPConfig, detail: None })
                }
            }
        }
        Ok(())
    }
}

// Create a Delivery Pipeline.
// Returns true if created, returns false if already exists.
pub fn create_delivery_pipeline(client: &APIClient, org: &String, proj: &String, pipe: &String) -> bool {
    if client.pipeline_exists(org, proj, pipe) {
        return false
    } else {
        client.create_pipeline(org, proj, pipe).unwrap();
        return true
    }
}

// Create a Delivery Project with Delivery as SCP (default).
// If the project is created, return true.
// If the project already exists, return false
pub fn create_delivery_project(client: &APIClient,
                               org: &String,
                               proj: &String) -> bool {
    if client.project_exists(org, proj) {
        return false
    } else {
        client.create_delivery_project(org, proj).unwrap();
        return true
    }
}

// Push local content to the Delivery Server if no upstream commits.
// Returns true if commits pushed, returns false if upstream commits found.
pub fn push_project_content_to_delivery() -> bool {
    if git::server_content() {
        return false
    } else {
        // TODO: move output up to init post --for bugfix.
        git::git_push_master().unwrap();
        return true
    }
}

// Create delivery remote if it doesn't exist. Returns true if created.
pub fn create_delivery_remote_if_missing(delivery_git_ssh_url: String) -> bool {
    if git::config_repo(&delivery_git_ssh_url, &project_path()).unwrap() {
        return true
    } else {
        return false
    }
}

// Check to see if the origin remote is set up.
pub fn check_github_remote() -> bool {
    let git_remote_result = git::git_command(&["remote"], &project_path());
    match git_remote_result {
        Ok(git_result) => {
            if !(git_result.stdout.contains("origin")) {
                return false
            }
            return true
        },
        Err(e) => panic!(e) // Unexpected error, raise.
    }
}

// Search for the project root directory
//
// We will walk through the provided path tree until we find the
// git config (`.git/config`) annd then we will extract the root
// directory.
//
// # Examples
//
// Having this directory tree:
// /delivery-cli
//  ├── .git
//  │   └── config
//  ├── src
//  │   └── delivery
//  └── features
//
// ```
// use std::env;
// use delivery::project::root_dir;
//
// let root = env::current_dir().unwrap();
//
// // Stepping into `delivery-cli/src/delivery`
// let mut delivery_src = env::current_dir().unwrap();
// delivery_src.push("src/delivery");
//
// assert_eq!(root, root_dir(&delivery_src.as_path()).unwrap());
// ```
pub fn root_dir(dir: &Path) -> Result<PathBuf, DeliveryError> {
    match walk_tree_for_path(&PathBuf::from(&dir), ".git/config") {
        Some(p) => {
           let git_d = p.parent().unwrap();
           let root_d = git_d.parent().unwrap();
           Ok(PathBuf::from(root_d))
        },
        None => Err(DeliveryError{kind: Kind::NoGitConfig,
                                  detail: Some(format!("current directory: {:?}",
                                                       dir))})
    }
}

pub fn project_path() -> PathBuf {
    root_dir(&utils::cwd()).unwrap()
}

// Return the project name from the current path
pub fn project_from_cwd() -> Result<String, DeliveryError> {
    let cwd = try!(self::root_dir(&utils::cwd()));
    Ok(cwd.file_name().unwrap().to_str().unwrap().to_string())
}

// Return the project name or try to extract it from the current path
pub fn project_or_from_cwd(proj: &str) -> Result<String, DeliveryError> {
    if proj.is_empty() {
        project_from_cwd()
    } else {
        Ok(proj.to_string())
    }
}

// Create the feature branch `add-delivery-config`
//
// This branch is created to start modifying the project repository
// In the case of a failure, we could roll back fearly easy by checking
// out master and deleting this feature branch.
//
// If feature branch created, return true, else return false.
pub fn create_feature_branch_if_missing(project_path: &PathBuf) -> bool {
    match git::git_command(&["checkout", "-b", "add-delivery-config"], project_path) {
        Ok(_) => {
            return true;
        },
        Err(e) => {
            match e.detail.clone() {
                Some(msg) => {
                    if msg.contains("A branch named 'add-delivery-config' already exists") {
                        git::git_command(&["checkout", "add-delivery-config"], project_path).unwrap();
                        return false;
                    } else {
                        // Unexpected error, raise.
                        panic!(e)
                    }
                },
                // Unexpected error, raise.
                None => panic!(e)
            }
        }
    }
}

// Add and commit the generated build-cookbook
pub fn add_commit_build_cookbook(custom_config_passed: &bool) -> () {
    // .delivery is probably not yet under version control, so we have to add
    // the whole folder instead of .delivery/build-cookbook.
    git::git_command(&["add", ".delivery"], &project_path()).unwrap();
    let mut commit_msg = "Adds Delivery build cookbook".to_string();
    if !(*custom_config_passed) {
        commit_msg = commit_msg + " and config";
    }
    git::git_command(&["commit", "-m", &commit_msg], &project_path()).unwrap();
    ()
}

pub fn create_dot_delivery() -> &'static Path {
    let dot_delivery = Path::new(".delivery");
    // Not expecting errors, no need to handle them.
    fs::create_dir_all(dot_delivery).unwrap();
    dot_delivery
}

pub fn create_default_build_cookbook() -> Output {
    let mut gen = utils::make_command("chef");
    gen.arg("generate")
        .arg("build-cookbook")
        .arg(".delivery/build-cookbook")
        .current_dir(&project_path());
    let output = gen.output().unwrap();
    output
}

#[derive(Debug)]
pub enum CustomCookbookSource {
    Cached,
    Disk,
    Git
}

// Custom build-cookbook generation
//
// This method handles a custom generator which could be:
// 1) A local path
// 2) Or a git repo URL
// TODO) From Supermarket
pub fn download_or_mv_custom_build_cookbook_generator(generator: &Path, cache_path: &Path) -> CustomCookbookSource {
    mkdir_recursive(cache_path).unwrap();
    if generator.has_root() {
        utils::copy_recursive(&generator, &cache_path).unwrap();
        return CustomCookbookSource::Disk
    } else {
        let cache_path_str = &cache_path.to_string_lossy();
        let generator_str = &generator.to_string_lossy();
        if is_dir(&cache_path) {
            return CustomCookbookSource::Cached
        } else {
            git::clone(&cache_path_str, &generator_str).unwrap();
            return CustomCookbookSource::Git
        }
    }
}

// Generate the build-cookbook using ChefDK generate
pub fn chef_generate_build_cookbook_from_generator(generator: &Path, project_path: &Path) -> Command {
    let mut command = utils::make_command("chef");
    command.arg("generate")
        .arg("cookbook")
        .arg(".delivery/build-cookbook")
        .arg("-g")
        .arg(generator)
        .current_dir(&project_path);
    command
}

// Default cookbooks generator cache path
pub fn generator_cache_path() -> Result<PathBuf, DeliveryError> {
    utils::home_dir(&[".delivery/cache/generator-cookbooks"])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use super::root_dir;

    #[test]
    fn detect_error_if_root_project_is_not_a_git_repo() {
        // This path doesn't even exist
        // So we will expect to throw an Err(_)
        let lib_path = Path::new("/project/src/libraries");
        match root_dir(&lib_path) {
            Ok(_) => assert!(false),
            Err(_) => assert!(true)
        }
    }
}
