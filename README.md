
# latexcompile

Latexcompile is a small rust crate which serves two aspects.
First, it is a library to utilize some latex compiler in a clean
temp enviroment. It provides basic templating features
and accepts files and binary streams.

Second, it provides a small REST service which accepts file streams
and generates pdf files with latexcompile.

## Context

The service should be used to integrate pdf generation facilities into
other programs. For example, some web program can use this service to
generate pdfs on-the-fly.

## Goals
### lib

* Provide a robust library to generate pdfs, if the input is valid.
* Provide a basic named templating. For example, `##a##` gets replaced at runtime by an some provided value. 
* easy interface
* Loops or other higher constructs are not part of the library.

### rest service

* Simple service to generate pdfs out of provided files
* Offer templating facility
* Actor model

## Milestones

1. Library prototype
2. web service prototype
3. testing (unit/integration)

## Solution

The library should have an easy interface.
The workflow of the library should be the following:
- Create a new LatexCompiler
- Provide the templating hashmap
- Provide the files or text streams as input
- recieve the ouput pdf as binary stream

The workflow of the rest service should be:
- The rest service is up and running
- It accepts files or text streams along with
  a key value hashmap as query param.
- It dispatches a new LatexCompiler and compiles the pdf
- The pdf is returned to the sender

## Notes

- Quite basic approach with limited templating
  and no search for installed latex compilers as
  well as configuring them.

## Timeline

7.11.18 Design document, take over of old code
