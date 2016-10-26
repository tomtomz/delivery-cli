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

use errors::{DeliveryError, Kind};
use std::convert::AsRef;
use std::fs;
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use utils::path_join_many::PathJoinMany;

pub mod say;
pub mod path_join_many;
pub mod path_ext;
pub mod open;

use crypto::digest::Digest;
use crypto::md5::Md5;

#[cfg(not(target_os = "windows"))]
pub use self::unix::*;

#[cfg(target_os = "windows")]
pub use self::windows::*;

#[cfg(not(target_os = "windows"))]
mod unix;

#[cfg(target_os = "windows")]
mod windows;

// Extract the Environment Variable of the provided `key`
pub fn env_variable(key: &str) -> Option<String> {
    env::var(key).ok()
}

pub fn mkdir_recursive<P: ?Sized>(path: &P) -> Result<(), DeliveryError> where P: AsRef<Path> {
    try!(fs::create_dir_all(path.as_ref()));
    Ok(())
}

pub fn home_dir(to_append: &[&str]) -> Result<PathBuf, DeliveryError>
{
   match env::home_dir() {
       Some(home) => Ok(home.join_many(to_append)),
       None => {
           let msg = "unable to find home dir".to_string();
           Err(DeliveryError{ kind: Kind::NoHomedir,
                              detail: Some(msg) })
       }
   }
}

/// Walk up a file hierarchy searching for `dir/target`.
pub fn walk_tree_for_path(dir: &Path, target: &str) -> Option<PathBuf> {
    let mut current = dir;
    loop {
        let candidate = current.join(target);
        if fs::metadata(&candidate).is_ok() {
            let ans = PathBuf::from(candidate);
            return Some(ans)
        }
        match current.parent() {
            Some(p) => current = p,
            None => return None
        }
    }
}

// Convert a path into a String. Panic if the path contains
// non-unicode sequences.
pub fn path_to_string<P: AsRef<Path>>(p: P) -> String {
    let path = p.as_ref();
    match path.to_str() {
        Some(s) => s.to_string(),
        None => {
            let s = format!("invalid path (non-unicode): {}",
                            path.to_string_lossy());
            panic!(s)
        }
    }
}

/// Return the content of the provided file
///
/// An easy way to read a file
///
/// # Examples
///
/// ```
/// use std::fs::{File, remove_file};
/// use std::io::prelude::*;
/// use std::path::PathBuf;
/// use delivery::utils::read_file;
///
/// let mut f = File::create("foo.txt").unwrap();
/// f.write_all(b"Cool beans!");
///
/// let f = PathBuf::from("foo.txt");
/// assert_eq!("Cool beans!", read_file(&f).unwrap());
///
/// remove_file("foo.txt");
/// ```
pub fn read_file(path: &PathBuf) -> Result<String, DeliveryError> {
    let mut buffer = String::new();
    let mut f = try!(File::open(path));
    try!(f.read_to_string(&mut buffer));
    Ok(buffer)
}

// Return the current directory path
pub fn cwd() -> PathBuf {
    env::current_dir().unwrap()
}

// Returns true if dest_f doesn't exist or has content different from source_f,
// returns false if dest_f exist but contains the exact content as source_f.
pub fn file_needs_updated(source_f: &PathBuf, dest_f: &PathBuf) ->Result<bool, DeliveryError> {
    if dest_f.exists() {
        let mut md5_source = Md5::new();            
        let mut source_f = try!(File::open(&source_f));
        let mut source_str = String::new();
        try!(source_f.read_to_string(&mut source_str));
        md5_source.input_str(&source_str);

        let mut md5_dest = Md5::new();
        let mut dest_f = try!(File::open(&dest_f));
        let mut dest_str = String::new();
        try!(dest_f.read_to_string(&mut dest_str));
        md5_dest.input_str(&dest_str);

        // If the md5 sun matches, return None to signify that
        // the file was not copied because they match exactly.
        if md5_source.result_str() == md5_dest.result_str() {
            return Ok(false)
        }
    }
    Ok(true)
}

/// Returns the absolute path for a given command, if it exists, by searching the `PATH`
/// environment variable.
///
/// If the command represents an absolute path, then the `PATH` seaching will not be performed. If
/// no absolute path can be found for the command, then `None` is returned.
///
/// On Windows, the PATHEXT environment variable contains common extensions for commands, 
/// for example allowing "docker.exe" to be found when searching for "docker".
pub fn find_command(command: &str) -> Option<PathBuf> {
    // If the command path is absolute and a file exists, then use that.
    let candidate = PathBuf::from(command);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate);
    }

    // Find the command by checking each entry in `PATH`. If we still can't find it, give up and
    // return `None`.
    match env::var_os("PATH") {
        Some(paths) => {
            for path in env::split_paths(&paths) {
                let candidate = PathBuf::from(&path).join(command);
                if candidate.is_file() {
                    return Some(candidate);
                } else {
                    match find_command_with_pathext(&candidate) {
                        Some(result) => return Some(result),
                        None => {}
                    }
                }
            }
            None
        }
        None => None,
    }
}

// Windows relies on path extensions to resolve commands like `docker` to `docker.exe`
// Path extensions are found in the PATHEXT environment variable.
// We should only search with PATHEXT if the file does not already have an extension.
fn find_command_with_pathext(candidate: &PathBuf) -> Option<PathBuf> {
    if candidate.extension().is_none() {
        match env::var_os("PATHEXT") {
            Some(pathexts) => {
                for pathext in env::split_paths(&pathexts) {
                    let mut source_candidate = candidate.to_path_buf();
                    let extension = pathext.to_str().unwrap().trim_matches('.');
                    source_candidate.set_extension(extension);
                    let current_candidate = source_candidate.to_path_buf();
                    if current_candidate.is_file() {
                        return Some(current_candidate);
                    }
                }
            }
            None => {},
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::walk_tree_for_path;
    use std::env;
    use std::path::PathBuf;
    use std::ffi::OsStr;

    #[test]
    fn traverse_up_for_dot_delivery_found() {
        let p = env::current_dir().unwrap();
        let result = walk_tree_for_path(&p, ".delivery");
        assert!(result.is_some());
        assert_eq!(Some(OsStr::new(".delivery")), result.unwrap().file_name());
    }

    #[test]
    fn traverse_up_for_dot_delivery_not_found() {
        // starting from / we don't expect to find .delivery
        let result = walk_tree_for_path(&PathBuf::from("/"), ".delivery-123");
        assert!(result.is_none());
    }
}
