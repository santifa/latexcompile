//
//    This file is part of latexcompile which serves as wrapper around
//    some latex compilerand provides a basic templating scheme.
//    Copyright (C) 2018  Henrik Jürges
//
//    This program is free software: you can redistribute it and/or modify
//    it under the terms of the GNU General Public License as published by
//    the Free Software Foundation, either version 3 of the License, or
//    (at your option) any later version.
//
//    This program is distributed in the hope that it will be useful,
//    but WITHOUT ANY WARRANTY; without even the implied warranty of
//    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//    GNU General Public License for more details.
//
//    You should have received a copy of the GNU General Public License
//    along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
//! # latexcompile
//!
//! This library provides a basic enviroment to produce a clean latex build.
//! It run the latex build within a `Tempdir`.
//!
//! It also provides a simple templating feature which can be used
//! to insert text fragements into the input files.
//!
//! ## Example
//!
//! ```
//! use std::collections::HashMap;
//! use std::fs::write;
//! use latexcompile::{LatexCompiler, LatexInput, LatexError};
//!
//! fn main() {
//!     // create the template map
//!     let mut dict = HashMap::new();
//!     dict.insert("test".into(), "Minimal".into());
//!     // provide the folder where the file for latex compiler are found
//!     let input = LatexInput::from("assets");
//!     // create a new clean compiler enviroment and the compiler wrapper
//!     let compiler = LatexCompiler::new(dict).unwrap();
//!     // run the underlying pdflatex or whatever
//!     let result = compiler.run("assets/test.tex", &input).unwrap();
//!
//!     // copy the file into the working directory
//!     let output = ::std::env::current_dir().unwrap().join("out.pdf");
//!     assert!(write(output, result).is_ok());
//! }
//! ```
//!
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate regex;
extern crate tempfile;

use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::{tempdir, TempDir};

/// Specify all error cases with the fail api.
#[derive(Fail, Debug)]
pub enum LatexError {
    #[fail(display = "General failure: {}.", _0)]
    LatexError(String),
    #[fail(display = "Failed to invoke latex compiler.")]
    CompilationError,
    #[fail(display = "Failed to change working enviroment.")]
    EnviromentError,
    #[fail(display = "Failed to create temporary context. {}", _0)]
    ContextCreationError(#[cause] std::io::Error),
    #[fail(display = "{}", _0)]
    Io(#[cause] std::io::Error),
    #[fail(display = "{}", _0)]
    Utf8(#[cause] std::str::Utf8Error),
}

/// result type alias idiom
type Result<T> = std::result::Result<T, LatexError>;

/// An alias for a command line
type Cmd = (String, Vec<String>);

/// The latex input provides the needed files
/// as tuple vector with name, buffer as tuple.
#[derive(Debug, PartialEq)]
pub struct LatexInput {
    input: Vec<(String, Vec<u8>)>
}

impl LatexInput {
    pub fn new() -> LatexInput {
        LatexInput {
            input: vec![],
        }
    }

    /// Add a single input tuple.
    /// ## Example
    /// ```
    /// # use latexcompile::{LatexCompiler, LatexInput, LatexError};
    /// fn main() {
    ///   let mut input = LatexInput::new();
    ///   input.add("name.tex", "test".as_bytes().to_vec());
    /// }
    /// ```
    pub fn add(&mut self, name: &str, buf: Vec<u8>) {
        self.input.push((name.into(), buf));
    }

    /// Add a single file as input.
    /// ## Example
    /// ```
    /// # use latexcompile::{LatexCompiler, LatexInput, LatexError};
    /// fn main() {
    ///   let mut input = LatexInput::from("assets/main.tex");
    ///   input.add("name.tex", "test".as_bytes().to_vec());
    /// }
    /// ```
    ///
    /// ## Note
    /// If the path is not a file or can't be converted to a string nothing is added and ok is returned.
    pub fn add_file(&mut self, file: PathBuf) -> Result<()> {
        if file.is_file() {
            match file.to_str() {
                Some(name) => {
                    let content = fs::read(&file).map_err(LatexError::Io)?;
                    self.input.push((name.to_string(), content));
                }
                None => {}
            }
        }
        Ok(())
    }

    /// Add a whole folder as input.
    /// ## Example
    /// ```
    /// # use latexcompile::{LatexCompiler, LatexInput, LatexError};
    /// fn main() {
    ///   let mut input = LatexInput::from("assets");
    ///   input.add("name.tex", "test".as_bytes().to_vec());
    /// }
    /// ```
    /// ## Note
    /// If the path is not a folder nothing is added.
    pub fn add_folder(&mut self, folder: PathBuf) -> Result<()> {
        if folder.is_dir() {
            let paths = fs::read_dir(folder).map_err(LatexError::Io)?;

            for path in paths {
                let p = path.map_err(LatexError::Io)?.path();
                if p.is_file() {
                    self.add_file(p);
                } else if p.is_dir() {
                    self.add_folder(p);
                }
            }
        }
        Ok(())
    }
}

/// Provide a simple From conversion for &str to latex input.
impl<'a> From<&'a str> for LatexInput {
    fn from(s: &'a str) -> LatexInput {
        let mut input = LatexInput::new();
        let path = PathBuf::from(s);
        if path.is_file() {
            input.add_file(path);
        } else if path.is_dir() {
            input.add_folder(path);
        }
        input
    }
}

/// Internal type alias for the key value store
type TemplateDict = HashMap<String, String>;

/// The processor takes latex files as input and replaces
/// matching placeholders (e.g. ##someVar##) with the real
/// content provided as HashMap.
struct TemplateProcessor {
    regex: Regex,
}

impl TemplateProcessor {
    /// Characters allowed as variable names: "a-zAZ0-9-_"
    fn new() -> Result<TemplateProcessor> {
        Ok(TemplateProcessor {
            regex: Regex::new(r"##[a-z|A-Z|\d|-|_]+##")
                .or(Err(LatexError::LatexError("Failed to compile regex.".to_string())))?,
        })
    }

    /// Replace placeholders with their actual value or nothing if no replacement
    /// is provided. The content is duplicated within this step.
    fn process_placeholders(
        &self,
        buf: &[u8],
        dict: &TemplateDict,
    ) -> Result<Vec<u8>> {
        if dict.is_empty() {
            return Ok(buf.into())
        }
        let mut replaced = String::new();
        let content = String::from_utf8_lossy(buf);
        let mut running_index = 0;
        for c in self.regex.captures_iter(&content) {
            let _match = c.get(0).unwrap();
            let key = &content[_match.start() + 2.._match.end() - 2];
            replaced += &content[running_index.._match.start()];

            match dict.get(key) {
                Some(value) => {
                    replaced += value;
                }
                None => {}
            }
            running_index = _match.end();
        }
        replaced += &content[running_index..];

        Ok(replaced.as_bytes().to_vec())
    }
}

/// The wrapper struct around some latex compiler.
/// It provides a clean temporary enviroment for the
/// latex compilation.
/// ```
/// use std::fs::write;
/// use std::collections::HashMap;
/// use latexcompile::{LatexCompiler, LatexInput, LatexError};
///
/// fn main() {
///    let compiler = LatexCompiler::new(HashMap::new()).unwrap();
///    let input = LatexInput::from("assets");
///    let pdf = compiler.run("assets/main.tex", &input);
///    assert!(pdf.is_ok());
/// }
/// ```
pub struct LatexCompiler {
    working_dir: TempDir,
    cmd: Cmd,
    tp: TemplateProcessor,
    dict: TemplateDict,
}

impl LatexCompiler {
    /// Create a new latex compiler wrapper
    pub fn new(dict: TemplateDict) -> Result<LatexCompiler> {
        let dir = tempdir().map_err(LatexError::Io)?;
        let cmd = ("pdflatex".into(), vec!["-interaction=nonstopmode".into()]);

        Ok(LatexCompiler {
            working_dir: dir,
            cmd: cmd,
            tp: TemplateProcessor::new()?,
            dict: dict,
        })
    }

    /// Overwrite the default command-line `pdflatex`
    pub fn with_cmd(mut self, cmd: &str) -> Self {
        self.cmd.0 = cmd.into();
        self
    }

    /// Clean the arguments list and add a new argument.
    /// Use add_arg to add further arguments
    pub fn with_args(mut self, cmd: &str) -> Self {
        self.cmd.1 = vec![cmd.into()];
        self
    }

    /// Add a new argument to the command-line.
    pub fn add_arg(mut self, cmd: &str) -> Self {
        self.cmd.1.push(cmd.into());
        self
    }

    /// build the command-line
    fn get_cmd(&self, main_file: &str) -> Command {
        let mut cmd = Command::new(&self.cmd.0);
        cmd.args(&self.cmd.1)
            .arg(main_file)
            .current_dir(self.working_dir.path());
        cmd
    }

    pub fn run(&self, main: &str, input: &LatexInput) -> Result<Vec<u8>> {
        // check if input is empty
        if input.input.is_empty() {
            return Err(LatexError::LatexError("No input files provided.".into()))
        }

        // apply the templating and create resources in the working dir
        for (name, buf) in &input.input {
            let transformed_buf = self.tp.process_placeholders(buf, &self.dict)?;
            let path = self.get_result_path(PathBuf::from(name))?;
            fs::write(path, transformed_buf).map_err(LatexError::Io)?;

        }

        // first and second run
        self.get_cmd(main).status().map_err(LatexError::Io)?;
        self.get_cmd(main).status().map_err(LatexError::Io)?;

        // get the output file
        let mut pdf = PathBuf::from(main); //self.get_result_path(PathBuf::from(main))?;
        let stem = PathBuf::from(pdf.file_stem().unwrap().to_str().unwrap());
        pdf = self.working_dir.path().join(stem.with_extension("pdf"));
        fs::read(pdf).map_err(LatexError::Io)
    }

    /// Create the given path as subpath within the working directory.
    fn get_result_path(&self, path: PathBuf) -> Result<PathBuf> {
        let dir = self.working_dir.path();
        let to_create = dir.join(path);
        match to_create.parent() {
            Some(p) => fs::create_dir_all(p).map_err(LatexError::Io)?,
            None => ()
        }
        Ok(to_create)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latex_input() {
        let name = "name.tex";
        let buf = "test".as_bytes();
        let expected = LatexInput{ input: vec![("name.tex".into(), "test".as_bytes().to_vec())]};
        let mut input = LatexInput::new();
        input.add(name, buf.to_vec());
        assert_eq!(input, expected);
    }

    #[test]
    fn test_latex_file_input() {
        let buf = include_bytes!("../assets/main.tex");
        let expected = LatexInput{ input: vec![("assets/main.tex".into(), buf.to_vec())]};
        let mut input = LatexInput::new();
        input.add_file(PathBuf::from("assets/main.tex"));
        assert_eq!(input, expected);
    }

    #[test]
    fn test_latex_folder_input() {
        let buf = include_bytes!("../assets/main.tex");
        let expected = LatexInput{ input: vec![("assets/nested/main.tex".into(), buf.to_vec())]};
        let mut input = LatexInput::new();
        input.add_folder(PathBuf::from("assets/nested"));
        assert_eq!(input, expected);
    }

    #[test]
    fn test_latex_complex_folder_input() {
        let buf1 = include_bytes!("../assets/main.tex");
        let buf2 = include_bytes!("../assets/logo.png");
        let buf3 = include_bytes!("../assets/test.tex");
        let buf4 = include_bytes!("../assets/card.tex");
        let buf5 = include_bytes!("../assets/nested/main.tex");
        let expected = LatexInput{
            input: vec![("assets/nested/main.tex".into(), buf5.to_vec()),
                        ("assets/test.tex".into(), buf3.to_vec()),
                        ("assets/main.tex".into(), buf1.to_vec()),
                        ("assets/logo.png".into(), buf2.to_vec()),
                        ("assets/card.tex".into(), buf4.to_vec())]
        };
        let mut input = LatexInput::new();
        input.add_folder(PathBuf::from("assets"));
        assert_eq!(input, expected);
    }

    #[test]
    fn test_empty_templating() {
        let templating = TemplateProcessor::new();
        assert!(templating.is_ok());
        let map = HashMap::new();
        let buf = include_bytes!("../assets/main.tex");
        let res = templating.unwrap().process_placeholders(buf, &map);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), buf.to_vec());
    }

    #[test]
    fn test_templating() {
        let templating = TemplateProcessor::new();
        assert!(templating.is_ok());
        let mut map = HashMap::new();
        map.insert("test".into(), "Minimal".into());

        let buf = include_bytes!("../assets/test.tex");
        let expected = include_bytes!("../assets/main.tex");
        let res = templating.unwrap().process_placeholders(buf, &map);
        assert!(res.is_ok());
        //println!("After:\n{}", String::from_utf8_lossy(&res.unwrap()));
        assert_eq!(res.unwrap(), expected.to_vec());
    }

    #[test]
    fn test_context_cmd() {
        let dict = HashMap::new();
        let wrapper = LatexCompiler::new(dict);
        assert!(wrapper.is_ok());
        let wrapper = wrapper.unwrap().with_cmd("latexmk").with_args("arg1").add_arg("arg2");
        let cmd = ("latexmk".into(), vec!["arg1".into(), "arg2".into()]);
        assert_eq!(wrapper.cmd, cmd);
    }

    #[test]
    fn test_path_replacement() {
        let wrapper = LatexCompiler::new(HashMap::new());
        assert!(wrapper.is_ok());
        let compiler = wrapper.unwrap();
        let expected = compiler.working_dir.path().join("assets/nested/main.tex");
        let path = compiler.get_result_path(PathBuf::from("assets/nested/main.tex"));
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), expected);
    }

    #[test]
    fn test_pdf_generation() {
        let compiler = LatexCompiler::new(HashMap::new()).unwrap();
        let input = LatexInput::from("assets");
        let pdf = compiler.run("assets/main.tex", &input);
        assert!(pdf.is_ok());
    }
}
