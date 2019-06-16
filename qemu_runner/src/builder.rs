use std::env;
use std::fs::File;
use std::fs::{read_dir, create_dir, copy, remove_dir_all, canonicalize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::ffi::OsString;

fn is_cargo(path: &str) -> bool {
	let output = Command::new(path).arg("-V").output();		
	if let Ok(output) = Command::new(path).arg("-V").output() {			
		let s = String::from_utf8_lossy(&output.stdout);
		if s.contains("cargo") {
			return true;
		}
	}

	false
}

fn find_cargo_path() -> Option<String> {	
	let mut p: Vec<String> = vec!["cargo".to_string()];
	if let Some(home) = env::home_dir() {
		p.push(format!("{}/.cargo/bin/cargo", home.display()));
	}

	for path in p {
		if is_cargo(&path) {
			return Some(path.into());
		}
	}

	None
}

#[derive(Clone, Debug)]
pub struct FoundFile {
	name: String,
	absolute_path: String
}

fn find_files<F: Fn(&str) -> bool>(dir: &str, filter: F) -> Vec<FoundFile> {
	let mut ret = vec![];

	let dir_absolute = canonicalize(&dir).expect(&format!("Couldn't find the absolute path of directory: {}", &dir));
	let dir_absolute_str = dir_absolute.to_str().unwrap();

	for entry in read_dir(&dir_absolute).expect(&format!("Directory not found: {}", &dir)) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            let name = entry.file_name().into_string().unwrap();
            if let Ok(name) = entry.file_name().into_string() {
            	if filter(&name) {
            		let absolute_path = format!("{}/{}", &dir_absolute_str, &name);

            		ret.push(FoundFile { name: name, absolute_path: absolute_path });
            	}
            }
        }
    }

    ret
}

#[derive(Debug)]
pub struct CrossbuildOptions {
	pub tests_project_path: String,
	pub target_arch: String
}


#[derive(Debug)]
pub struct CrossbuiltTests {
	pub object_paths: Vec<String>,
	pub tests: Vec<String>,
	pub library_path: String
}

pub fn crossbuild_rust_tests(options: &CrossbuildOptions) -> CrossbuiltTests {

	// check if we can find cargo for cross building
	let cargo_path = find_cargo_path();
	let cargo_path = cargo_path.expect("Cargo not found! Install Rust's package manager.");

	let build_proj_root = {
		let p = Path::new(&options.tests_project_path);
		let mut absolute_path = ::std::env::current_dir().expect("Can't find current dir?");
		absolute_path.push(p);
		p.canonicalize().expect("Error canonicalizing")
	};

	// grab the list of tests to compile binaries for
	let tests: Vec<_> = {
		let dir = format!("{}/examples/", &options.tests_project_path);
		let tests = find_files(&dir, |n| {
			n.starts_with("test_") && n.ends_with(".rs")
		}).iter().cloned().map(|f| f.name).map(|n| { n.replace(".rs", "") }).collect();

		tests
	};
	let mut built_tests = vec![];

	for test in &tests {
		// cross-build the tests library
		let cargo_build = Command::new(&cargo_path)
					.current_dir(&options.tests_project_path)
					.arg("build")

					.arg("--example")
					.arg(test)
					
					.arg("--verbose")
					
					.arg("--target")
					.arg(&options.target_arch)
					
					//.env("RUSTFLAGS", "-C linker=arm-none-eabi-gcc -Z linker-flavor=gcc") 

					.env("CARGO_INCREMENTAL", "0")
					//.env("RUSTFLAGS", "--emit=obj")
					//.env("RUST_TARGET_PATH", &build_proj_root.to_str().expect("Missing path to proj root for target path?"))
					
					.stdout(Stdio::inherit())
					.stderr(Stdio::inherit())
					.output();

		let output = cargo_build.expect("Cargo build of the tests projects failed");
		if !output.status.success() {
			panic!("Cargo build failed");
		}

		built_tests.push(test.clone());
	}

	let library_path = {
		let p = format!("{}/target/{}/debug/examples/", &options.tests_project_path, &options.target_arch);
		let p = Path::new(&p);
		p.canonicalize().unwrap().to_str().unwrap().into()
	};
	
	CrossbuiltTests {
		object_paths: vec![],
		tests: built_tests,
		library_path: library_path
	}
}

#[derive(Debug, Clone)]
pub struct Stm32Test {
	pub name: String,
	pub absolute_elf_path: String
}

#[derive(Debug, Clone)]
pub struct Stm32Binaries {
	pub binaries: Vec<Stm32Test>
}

pub fn build_test_binaries(options: &CrossbuildOptions, tests: &CrossbuiltTests) -> Stm32Binaries {

	let mut binaries = vec![];
	let gcc_proj_dir = format!("{}/gcc/", options.tests_project_path);
	let test_objects = tests.object_paths.join(" ");

	for test in &tests.tests {
		let mut test_renames = "".to_string();

		/*
		if test.contains("isr_timer4") {
			test_renames.push_str(&format!("testbed_timer4_isr = {}_timer4_isr;", test));
		}
		*/


		let mut test_deps = vec![
			format!("{}/lib{}.a", &tests.library_path, &test)
		];

		let test_binary_build = Command::new("make")
				.current_dir(&gcc_proj_dir)
				.env("TEST_NAME", test.clone())
				.env("TEST_LIBRARY_PATH", format!("-L {}", &tests.library_path))
				.env("TEST_LIBRARY_PRE", format!("-l:lib{}.a", &test))
				.env("TEST_OBJECTS", &test_objects)
				.env("TEST_DEPS", test_deps.join(" "))
				.env("TEST_RENAMES", test_renames)
				.stdout(Stdio::inherit())
				.stderr(Stdio::inherit())
				.output();
		let output = test_binary_build.unwrap();
		if !output.status.success() {
			panic!(format!("GCC ARM build for '{}' failed", test));
		}

		binaries.push(Stm32Test {
			name: test.clone(),
			absolute_elf_path: canonicalize(&format!("{}/build/stm32_{}.elf", &gcc_proj_dir, &test)).unwrap().to_str().unwrap().into()
		});
	}

	Stm32Binaries {
		binaries: binaries
	}
}