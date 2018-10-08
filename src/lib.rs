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
//! fn main() {
//!     // create the template map
//!     let mut template = HashMap::new();
//!     // provide the folder where the file for latex compiler are found
//!     let template_folder = Path::new("template_folder").to_path_buf();
//!     // create a new clean compiler enviroment
//!     let compiler = LatexCompiler::new(template_folder, None, "main.tex").unwrap();
//!     // run the underlying pdflatex or whatever
//!     let result_path = compiler.run("example.pdf", &template);
//!
//!     // copy the file into the working directory
//!     let output = current_dir.join(output_name);
//!     copy(result.clone(), output.clone()).chain_err(|| {
//!         format!(
//!             "Unable to copy result {:?} to location {:?}",
//!             result, output
//!         )
//!     })?;
//! }
//! ```
//!
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate log;
extern crate regex;
extern crate tempfile;

use failure::{err_msg, Error};
use regex::Regex;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::fs::{copy, read_dir};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{tempdir, TempDir};

#[derive(Debug)]
struct Cmd {
    cmd: String,
    args: Vec<String>,
}

impl Default for Cmd {
    fn default() -> Self {
        Cmd {
            cmd: "".into(),
            args: vec!["".into()],
        }
    }
}
// .arg("-interaction=nonstopmode")
//            .arg(&self.ctx.main_file)

/// Provide the context informations for the templating
/// and compile process.
#[derive(Debug)]
struct Context {
    working_dir: TempDir,
    source_dir: PathBuf,
    main_file: String, // The main file for latex
    cmd: Cmd,
}

impl Context {
    /// Create a new LatexCompiler which uses the
    fn new(source: PathBuf, cli: Option<String>, file: &str) -> Result<Context> {
        let dir = tempdir().chain_err(|| "Unable to create temporary enviroment.")?;
        Ok(Context {
            working_dir: dir,
            cmd: cli.unwrap_or("pdflatex".into()),
            source_dir: source,
            main_file: file.into(),
        })
    }
}

/// This struct defines a clean enviroment in which
/// the latex files are compiled. It takes a command line, how call latex
/// and provides helper for getting the result back and cleaning up the enviroment.
pub struct LatexCompiler {
    ctx: Context,
    tp: TemplateProcessor,
}

impl LatexCompiler {
    pub fn new(source: PathBuf, cli: Option<String>, file: &str) -> Result<LatexCompiler> {
        Ok(LatexCompiler {
            ctx: Context::new(source, cli, file)?,
            tp: TemplateProcessor::new()?,
        })
    }

    pub fn run(&self, output_name: &str, dict: &HashMap<String, String>) -> Result<()> {
        // prepare sources
        self.tp.process_sources(&self.ctx, &dict);

        // change into new working directory
        let current_dir = ::std::env::current_dir().chain_err(|| "Unable to get current dir")?;
        ::std::env::set_current_dir(self.ctx.working_dir.path())
            .chain_err(|| "Unable to switch to enviroment.")?;

        // run latex on main pdf
        Command::new(&self.ctx.cmd)
            .arg("-interaction=nonstopmode")
            .arg(&self.ctx.main_file)
            .status()
            .chain_err(|| "Failed to run latex.")?;

        // do a rerun
        Command::new(&self.ctx.cmd)
            .arg("-interaction=nonstopmode")
            .arg(&self.ctx.main_file)
            .status()
            .chain_err(|| "Failed to run latex.")?;

        // get name of the result file
        let base = Path::new(&self.ctx.main_file)
            .file_stem()
            .and_then(OsStr::to_str)
            .chain_err(|| "Unable to get base name.")?;
        let result = self.ctx.working_dir.path().join(base.to_string() + ".pdf");

        // return result
        let output = current_dir.join(output_name);
        copy(result.clone(), output.clone()).chain_err(|| {
            format!(
                "Unable to copy result {:?} to location {:?}",
                result, output
            )
        })?;

        // reset env
        ::std::env::set_current_dir(current_dir).chain_err(|| "Unable to switch Enviroment back.")
    }

    /*  /// This is a handy method to add partial documents to the enviroment.
    /// These are excluded from template processing.
    pub fn write_latex_document(&self, doc: Document, name: &str) -> Result<()> {
        let file = self.ctx.working_dir.path().join(name);
        let rendered = print(&doc).chain_err(|| "Unable to render file.")?;
        let mut f = File::create(file).chain_err(|| "Unable to open file.")?;
        write!(f, "{}", rendered).chain_err(|| "Unable to write file.")?;
        Ok(())
    }*/
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
            regex: Regex::new(r"##[a-z|A-Z|\d|-|_]+##")
                .chain_err(|| "Unable to compile variabel regex")?,
        })
    }

    /// Replace variables for all files within the template path and
    /// copy the results into the created enviroment.
    fn process_sources(&self, ctx: &Context, dict: &HashMap<String, String>) -> Result<()> {
        let paths = read_dir(&ctx.source_dir).chain_err(|| "Failed to read template directory.")?;
        for path in paths {
            let src_file = path.chain_err(|| "Unable to get source file path.")?.path();
            let dst_file = ctx.working_dir.path().join(
                src_file
                    .strip_prefix(&ctx.source_dir)
                    .chain_err(|| "Unable to strip common prefix.")?,
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
        let mut src_file = File::open(src).chain_err(|| "Unable to open source file.")?;

        match src_file.read_to_string(&mut content) {
            Err(_) => {
                // maybe binary data, so just copy it
                copy(&src, &dst).chain_err(|| "Unable to copy file.")?;
            }
            Ok(_) => {
                let replaced_content = self.process_placeholders(&content, &dict)?;
                File::create(dst)
                    .and_then(|mut f| f.write_all(replaced_content.as_bytes()))
                    .chain_err(|| "Unable to create destination file.")?;
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
            let _match = c.get(0).chain_err(|| "Unable to get regex match.")?;
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
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
