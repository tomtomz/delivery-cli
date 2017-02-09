//
// Copyright:: Copyright (c) 2016 Chef Software, Inc.
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
use cli::arguments::{pipeline_arg, patchset_arg,
                     value_of, project_specific_args};
use clap::{App, SubCommand, ArgMatches};
use cli::{CommandPrep, merge_fips_options_and_config};
use types::DeliveryResult;
use config::Config;

pub const SUBCOMMAND_NAME: &'static str = "diff";

#[derive(Debug)]
pub struct DiffClapOptions<'n> {
    pub change: &'n str,
    pub patchset: &'n str,
    pub pipeline: &'n str,
    pub local: bool,
    pub fips: bool,
    pub fips_git_port: &'n str,
}

impl<'n> Default for DiffClapOptions<'n> {
    fn default() -> Self {
        DiffClapOptions {
            change: "",
            patchset: "",
            pipeline: "master",
            local: false,
            fips: false,
            fips_git_port: "",
        }
    }
}

impl<'n> DiffClapOptions<'n> {
    pub fn new(matches: &'n ArgMatches<'n>) -> Self {
        DiffClapOptions {
            change: value_of(&matches, "change"),
            patchset: value_of(&matches, "patchset"),
            pipeline: value_of(&matches, "pipeline"),
            local: matches.is_present("local"),
            fips: matches.is_present("fips"),
            fips_git_port: value_of(&matches, "fips-git-port"),
        }
    }
}

impl<'n> CommandPrep for DiffClapOptions<'n> {
    fn merge_options_and_config(&self, config: Config) -> DeliveryResult<Config> {
        let new_config = config.set_pipeline(&self.pipeline);
        merge_fips_options_and_config(self.fips, self.fips_git_port, new_config)
    }

    fn initialize_command_state(&self, config: Config) -> DeliveryResult<Config> {
        if self.local {
            return Ok(config)
        }
        self.init_project_specific(config)
    }
}

pub fn clap_subcommand<'c>() -> App<'c, 'c> {
    SubCommand::with_name(SUBCOMMAND_NAME)
        .about("Display diff for a change")
        .args(&vec![patchset_arg()])
        .args(&pipeline_arg())
        .args_from_usage(
            "<change> 'Name of the feature branch to compare'
            -l --local \
            'Diff against the local branch HEAD'")
        .args(&project_specific_args())
}
