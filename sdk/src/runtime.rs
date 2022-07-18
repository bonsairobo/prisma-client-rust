use std::{
    fs::{self, File},
    io::{stderr, stdin, BufRead, BufReader, Write},
    path::Path,
};

use datamodel::parse_schema;
use serde_json::{Map, Value};

use crate::{
    args::GenerateArgs,
    dmmf::EngineDMMF,
    jsonrpc, prisma_cli,
    utils::{build_schema, rustfmt, validate_names},
};

pub struct GeneratorMetadata {
    generate_fn: fn(GenerateArgs, Map<String, Value>) -> String,
    name: &'static str,
    default_output: &'static str,
}

impl GeneratorMetadata {
    pub fn new(
        generate_fn: fn(GenerateArgs, Map<String, Value>) -> String,
        name: &'static str,
        default_output: &'static str,
    ) -> Self {
        Self {
            generate_fn,
            name,
            default_output,
        }
    }
}

pub fn run_generator(generator: GeneratorMetadata, args: &Vec<String>) {
    if args.len() > 0 {
        prisma_cli::main(args);
        return;
    }

    if let Err(_) = std::env::var("PRISMA_GENERATOR_INVOCATION") {
        println!(
            "This command is only meant to be invoked internally. Please specify a command to run."
        );

        std::process::exit(1);
    }

    loop {
        let mut content = String::new();
        BufReader::new(stdin())
            .read_line(&mut content)
            .expect("Failed to read Prisma engine output");

        let input: jsonrpc::Request = serde_json::from_str(&content).unwrap();

        let value = match input.method.as_str() {
            "getManifest" => serde_json::to_value(jsonrpc::ManifestResponse {
                manifest: jsonrpc::Manifest {
                    default_output: generator.default_output.to_string(),
                    pretty_name: generator.name.to_string(),
                    ..Default::default()
                },
            })
            .unwrap(),
            "generate" => {
                let params_str = input.params.to_string();

                let deserializer = &mut serde_json::Deserializer::from_str(&params_str);

                let dmmf = serde_path_to_error::deserialize(deserializer)
                    .expect("Failed to deserialize DMMF from Prisma engines");

                generate(&generator, dmmf);

                serde_json::Value::Null
            }
            method => panic!("Unknown generator method {}", method),
        };

        let response = jsonrpc::Response {
            jsonrpc: "2.0".to_string(),
            id: input.id,
            result: value,
        };

        let mut bytes =
            serde_json::to_vec(&response).expect("Could not marshal json data for reply");

        bytes.push(b'\n');

        stderr()
            .by_ref()
            .write(bytes.as_ref())
            .expect("Failed to write output to stderr for Prisma engines");

        if input.method.as_str() == "generate" {
            break;
        }
    }
}

fn generate(generator: &GeneratorMetadata, dmmf: EngineDMMF) {
    let (configuration, datamodel) =
        parse_schema(&dmmf.datamodel).expect("Failed to parse datamodel");
    let schema = build_schema(&datamodel, &configuration);

    let output_str = dmmf.generator.output.get_value();
    let output_path = Path::new(&output_str);

    let mut file = create_generated_file(&output_path);

    let args = GenerateArgs::new(datamodel, schema, dmmf.datamodel, dmmf.datasources);

    validate_names(&args);

    let generated_str = (generator.generate_fn)(args, dmmf.generator.config);

    file.write(format!("// Code generated by {}. DO NOT EDIT\n\n", generator.name).as_bytes())
        .expect("Failed to write file header");

    file.write(generated_str.as_bytes())
        .expect("Failed to write generated code");

    rustfmt(output_path);
}

fn create_generated_file(path: &Path) -> File {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("Failed to create output directory");
    }

    File::create(&path).expect("Failed to open output file")
}
