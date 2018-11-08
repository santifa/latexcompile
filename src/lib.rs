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
//!     // provide the folder where the file for latex compiler are found
//!     let input = LatexInput::from("assets");
//!     // create a new clean compiler enviroment and the compiler wrapper
//!     let compiler = LatexCompiler::new(dict).unwrap();
//!     // run the underlying pdflatex or whatever
//!     let result = compiler.run("main.tex", &input).unwrap();
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

use regex::bytes::Regex;
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
        content: &[u8],
        dict: &TemplateDict,
    ) -> Result<Vec<u8>> {
        if !dict.is_empty() {
            return Ok(content.into())
        }
        let mut replaced = vec![];

        let mut running_index = 0;
        for c in self.regex.captures_iter(content) {
            let _match = c.get(0).unwrap();
            //ok_or(Err(CompilerError::TemplatingError("Unable to get regex match.".to_string())))?;
            let key = &content[_match.start() + 2.._match.end() - 2];
            replaced.extend_from_slice(&content[running_index.._match.start()]);
            println!("found {:?}\n", key);

            let key_str = &std::str::from_utf8(key).map_err(LatexError::Utf8)?;
            match dict.get(*key_str) {
                Some(value) => {
                    replaced.extend_from_slice(value.as_bytes());
                }
                None => {}
            }
            running_index = _match.end();
        }
        replaced.extend_from_slice(&content[running_index..]);

        Ok(replaced)
    }
}
/// The wrapper struct around some latex compiler.
/// It provides a clean temporary enviroment for the
/// latex compilation.
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
        // prepare sources
        Err(LatexError::LatexError("No input files provided.".into()))
    }
}
 /*       for file in files.iter() {
            let source_dir = source_path.unwrap_or(|| {
                let source = Path::new(&files[0]);
                source.is_dir() {
                    source
                } else {
                    source.parent().unwrap_or(Path::new("/"))
                }
            });
       //     self.preprocess_input(file, source);
        }

        // first and second run
        self.ctx.get_cmd().status().map_err(CompilerError::Io)?;
        self.ctx.get_cmd().status().map_err(CompilerError::Io)?;

        // get name of the result file
        let result_name = self.ctx.get_result_name(suffix.unwrap_or(".pdf")).ok_or(CompilerError::CompilationError)?;

        // copy result file
        // let output = ::std::env::current_dir().map(|dir| dir.join(output_name)).map_err(CompilerError::Io)?;
        // copy(result_name, output)
        //     .map_err(CompilerError::Io)?;

        Ok(self.ctx.working_dir.path().join(result_name))
*/
 //   }
/*
    /// The preprocessing copies the provided files or folder structures
    /// into the temporary working directory. Normal text files gets checked
    /// for replacements by the templating processor.
    fn preprocess_input(&self, file: &PathBuf, source_dir: &PathBuf) -> Result<()> {
        let path = Path::new(file);
        let metadata = path.metadata().expect("metadata call failed");
        let destination = self.ctx.working_dir.path().join(
            src_file
                .strip_prefix(self.ctx.source_dir)
                .or(Err(CompilerError::TemplatingError("Unable to strip prefix.".to_string())))?
        );

        if path.is_file() {
            self.preprocess_file(&path, &destination)?;

        } else if path.is_dir() {
let paths = read_dir(path)
.or(Err(CompilerError::TemplatingError(format!("Failed to read directory {:?}.", path).to_string())))?;
            create_dir(destination).map_err(CompilerError::Io)?;
            for path in paths {
                    let src_file = path
                    .or(Err(CompilerError::TemplatingError("Unable to get source file path.".to_string())))?.path();
                self.preprocess_input(&src_file, source_dir)?;
            }
        } else {
            Error(CompilerError::TemplatingError("Neither a file nor a directory.".into()))
        }
        Ok(())
    }

    fn preprocess_file(&self, path: &Path, destination: &Path) -> Result<()> {
        let mut content = String::new();
        let mut src_file = File::open(path)
            .or(Err(CompilerError::TemplatingError("Unable to open source file.".to_string())))?;

        match src_file.read_to_string(&mut content) {
            Err(_) => {
                // maybe binary data, so just copy it
                copy(&src, &dst).map_err(CompilerError::Io)?;
                //.or(Err(CompilerError::TemplatingError("Unable to copy file.".to_string())))?;
            }
            Ok(_) => {
                let replaced_content = self.tp.process_placeholders(&content, &self.dict)?;
                //                        self.tp.process_sources(&self.ctx, &self.dict, files)?;
                File::create(dst)
                    .and_then(|mut f| f.write_all(replaced_content.as_bytes()))
                    .or(Err(CompilerError::TemplatingError("Unable to create destination file.".to_string())))?;
            }
        }
        Ok(())
    }*/


/*
    /// Replace variables for all files within the template path and
    /// copy the results into the created enviroment.
    // TODO Handle folders
    fn process_sources(&self, ctx: &Context, dict: &HashMap<String, String>, files: &[u8]) -> Result<()> {
        let paths = read_dir(&ctx.source_dir)
            .or(Err(CompilerError::TemplatingError("Failed to read template directory.".to_string())))?;
        for path in paths {
            let src_file = path
                .or(Err(CompilerError::TemplatingError("Unable to get source file path.".to_string())))?.path();
    let dst_file = ctx.working_dir.path().join(
    src_file
    .strip_prefix(&ctx.source_dir)
    .or(Err(CompilerError::TemplatingError("Unable to strip prefix.".to_string())))?
);

    self.process_file(&src_file, &dst_file, &dict)?;
}

        Ok(())
    }

    /// Process a single file. If the file is a non-text file it is copied into the
    /// destination enviroment, otherwise all placeholders are replaced with their
    /// actual value.
    fn process_file(&self, src: &Path, dst: &Path, dict: &HashMap<String, String>) -> Result<()> {
        let mut content = String::new();
        let mut src_file = File::open(src)
            .or(Err(CompilerError::TemplatingError("Unable to open source file.".to_string())))?;

        match src_file.read_to_string(&mut content) {
            Err(_) => {
                // maybe binary data, so just copy it
                copy(&src, &dst).map_err(CompilerError::Io)?;//.or(Err(CompilerError::TemplatingError("Unable to copy file.".to_string())))?;
            }
            Ok(_) => {
                let replaced_content = self.process_placeholders(&content, &dict)?;
                File::create(dst)
                    .and_then(|mut f| f.write_all(replaced_content.as_bytes()))
                    .or(Err(CompilerError::TemplatingError("Unable to create destination file.".to_string())))?;
            }
        }

        Ok(())
    }

    /// Replace placeholders with their actual value or nothing if no replacement
    /// is provided. The content is duplicated within this step.
    fn process_placeholders(
        &self,
        content: &str,
        dict: &HashMap<String, String>,
    ) -> Result<String> {
        if !dict.is_empty() {
            return Ok(content.into())
        }
        let mut replaced = String::new();

        let mut running_index = 0;
        for c in self.regex.captures_iter(&content) {
            let _match = c.get(0).unwrap();
            //ok_or(Err(CompilerError::TemplatingError("Unable to get regex match.".to_string())))?;
            let key = &content[_match.start() + 2.._match.end() - 2];
            replaced += &content[running_index.._match.start()];
            println!("found {:?}\n", key);

            match dict.get(key) {
                Some(value) => {
                    replaced += value;
                }
                None => {}
            }
            running_index = _match.end();
        }
        replaced += &content[running_index..];

        Ok(replaced)
    }*/
//}


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
        let buf3 = include_bytes!("../assets/card.tex");
        let buf4 = include_bytes!("../assets/nested/main.tex");
        let expected = LatexInput{
            input: vec![("assets/nested/main.tex".into(), buf4.to_vec()),
                        ("assets/main.tex".into(), buf1.to_vec()),
                        ("assets/logo.png".into(), buf2.to_vec()),
                        ("assets/card.tex".into(), buf3.to_vec())]
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

    /*
    #[test]
    fn test_context_cmd() {
        let mut context = Context::new(PathBuf::new(), "".into());
        assert!(context.is_ok());
        let context = context.unwrap().with_cmd("latexmk").with_args("arg1").add_arg("arg2");
        let ctx = ("latexmk".into(), vec!["arg1".into(), "arg2".into()]);
        assert_eq!(context.cmd, ctx);
    }
*/
}
