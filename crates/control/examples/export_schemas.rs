use std::{env, fs, path::Path};

use quantick_control::schema_catalog::public_contract_documents;

fn main() {
    let requested = env::args().nth(1);
    let documents = public_contract_documents();
    if requested.as_deref() == Some("--patch") {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("control crate is two levels below the workspace root");
        println!("*** Begin Patch");
        for document in documents {
            let path = root.join("schemas/control").join(document.file_name);
            let json = format!(
                "{}\n",
                serde_json::to_string_pretty(&document.document).unwrap()
            );
            match fs::read_to_string(&path) {
                Ok(previous)
                    if serde_json::from_str::<serde_json::Value>(&previous).ok()
                        == Some(document.document.clone()) =>
                {
                    continue;
                }
                Ok(previous) => {
                    println!("*** Update File: {}", path.display());
                    println!("@@");
                    for line in previous.lines() {
                        println!("-{line}");
                    }
                    for line in json.lines() {
                        println!("+{line}");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    println!("*** Add File: {}", path.display());
                    for line in json.lines() {
                        println!("+{line}");
                    }
                }
                Err(error) => panic!("cannot read {}: {error}", path.display()),
            }
        }
        println!("*** End Patch");
        return;
    }

    for document in documents {
        if requested
            .as_deref()
            .is_none_or(|name| name == document.file_name)
        {
            println!("# {}", document.file_name);
            println!(
                "{}",
                serde_json::to_string_pretty(&document.document).unwrap()
            );
        }
    }
}
