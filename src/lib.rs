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
//! use std::fs::copy;
//! use std::path::{Path, PathBuf};
//! use latexcompile::{LatexCompiler, Context, CompilerError};
//! fn main() {
//!     // create the template map
//!     let mut template = HashMap::new();
//!     // provide the folder where the file for latex compiler are found
//!     let template_folder = PathBuf::from("assets");
//!     // create a new clean compiler enviroment and the compiler wrapper
//!     let context = Context::new(template_folder, "card.tex").unwrap();
//!     let compiler = LatexCompiler::new(context).unwrap();
//!     // run the underlying pdflatex or whatever
//!     let result = compiler.run(None, &template).unwrap();
//!
//!     // copy the file into the working directory
//!     let output = ::std::env::current_dir().unwrap().join("out.pdf");
//!     copy(result.clone(), output.clone());
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
use std::ffi::OsStr;
use std::fs::File;
use std::fs::{copy, read_dir};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{tempdir, TempDir};

/// Specify all error cases with the fail api.
#[derive(Fail, Debug)]
pub enum CompilerError {
    #[fail(display = "Failed to apply template at stage {}.", _0)]
    TemplatingError(String),
    #[fail(display = "Failed to invoke latex compiler.")]
    CompilationError,
    #[fail(display = "Failed to change working enviroment.")]
    EnviromentError,
    #[fail(display = "Failed to create temporary context. {}", _0)]
    ContextCreationError(#[cause] std::io::Error),
    #[fail(display = "{}", _0)]
    Io(#[cause] std::io::Error),
}

/// result type alias idiom
type Result<T> = std::result::Result<T, CompilerError>;

/// Hold the command line program which is used and the required arguments.
/// As default `pdflatex -interaction=nonstopmode` is used.
#[derive(Debug, PartialEq)]
struct Cmd {
    cmd: String,
    args: Vec<String>,
}

impl Default for Cmd {
    fn default() -> Self {
        Cmd {
            cmd: "pdflatex".into(),
            args: vec!["-interaction=nonstopmode".into()],
        }
    }
}

/// The context provides the clean envoriment for a compile process.
/// It should be created new for every run since it destroys the
/// temporary working directory.
#[derive(Debug)]
pub struct Context {
    working_dir: TempDir,
    source_dir: PathBuf,
    main_file: String, // The main file for latex
    cmd: Cmd,
}

impl Context {
    /// Create a new basic context or throw an error if we have no access to the temp directory.
    /// Provide only the file name as main_file which can be found under the template source directory.
    pub fn new(source: PathBuf, main_file: &str) -> Result<Context> {
        let dir = tempdir().map_err(CompilerError::Io)?;
        Ok(Context {
            working_dir: dir,
            cmd: Cmd::default(),
            source_dir: source,
            main_file: main_file.into(),
        })
    }

    /// Overwrite the default cmd `pdflatex`
    pub fn with_cmd(mut self, cmd: &str) -> Self {
        self.cmd.cmd = cmd.into();
        self
    }

    /// Clean the arguments list and add a new argument.
    /// Use add_arg to add further arguments
    pub fn with_args(mut self, cmd: &str) -> Self {
        self.cmd.args = vec![cmd.into()];
        self
    }

    /// Add a new argument to the cmd.
    pub fn add_arg(mut self, cmd: &str) -> Self {
        self.cmd.args.push(cmd.into());
        self
    }

    /// build the command line
    fn get_cmd(&self) -> Command {
        let mut cmd = Command::new(&self.cmd.cmd);
        cmd.args(&self.cmd.args)
            .arg(&self.main_file)
            .current_dir(self.working_dir.path());
        cmd
    }

    /// get the name of the result file
    fn get_result_name(&self, suffix: &str) -> Option<PathBuf> {
        Path::new(&self.main_file).file_stem().and_then(OsStr::to_str)
            .and_then(|name| Some(self.working_dir.path().join(name.to_string() + suffix)))
    }
}

/// The latex compiler wrapper which includes
/// a template processor.
pub struct LatexCompiler {
    ctx: Context,
    tp: TemplateProcessor,
}

impl LatexCompiler {
    pub fn new(ctx: Context) -> Result<LatexCompiler> {
        Ok(LatexCompiler {
            ctx: ctx,
            tp: TemplateProcessor::new()?,
        })
    }

    pub fn run(&self, suffix: Option<&str>, dict: &HashMap<String, String>) -> Result<PathBuf> {
        // prepare sources
        self.tp.process_sources(&self.ctx, &dict);

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
    }
}

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
            regex: Regex::new(r"##[a-z|A-Z|\d|-|_]+##").or(Err(CompilerError::TemplatingError("Failed to compile regex.".to_string())))?,
        })
    }

    /// Replace variables for all files within the template path and
    /// copy the results into the created enviroment.
    fn process_sources(&self, ctx: &Context, dict: &HashMap<String, String>) -> Result<()> {
        let paths = read_dir(&ctx.source_dir).or(Err(CompilerError::TemplatingError("Failed to read template directory.".to_string())))?;
        for path in paths {
            let src_file = path.or(Err(CompilerError::TemplatingError("Unable to get source file path.".to_string())))?.path();
            let dst_file = ctx.working_dir.path().join(
                src_file
                    .strip_prefix(&ctx.source_dir).or(Err(CompilerError::TemplatingError("Unable to strip prefix.".to_string())))?
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
        let mut src_file = File::open(src).or(Err(CompilerError::TemplatingError("Unable to open source file.".to_string())))?;

        match src_file.read_to_string(&mut content) {
            Err(_) => {
                // maybe binary data, so just copy it
                copy(&src, &dst).or(Err(CompilerError::TemplatingError("Unable to copy file.".to_string())))?;
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
        let mut replaced = String::new();

        let mut running_index = 0;
        for c in self.regex.captures_iter(&content) {
            let _match = c.get(0).unwrap(); //ok_or(Err(CompilerError::TemplatingError("Unable to get regex match.".to_string())))?;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_cmd() {
        let mut context = Context::new(PathBuf::new(), "".into());
        assert!(context.is_ok());
        let context = context.unwrap().with_cmd("latexmk").with_args("arg1").add_arg("arg2");
        let ctx = Cmd {
            cmd: "latexmk".into(),
            args: vec!["arg1".into(), "arg2".into()],
        };
        assert_eq!(context.cmd, ctx);
    }

    #[test]
    fn test_templating() {
        let assets = PathBuf::from("assets");
        let context = Context::new(assets, "card.tex".into());
        assert!(context.is_ok());
        let templating = TemplateProcessor::new();
        assert!(templating.is_ok());
        let map = HashMap::new();
        let res = templating.unwrap().process_sources(&context.unwrap(), &map);
        assert!(res.is_ok());
    }
}
