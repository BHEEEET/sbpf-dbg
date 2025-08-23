use anyhow::{Error, Result};
use dirs::home_dir;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub const DEFAULT_LINKER: &str = r#"PHDRS
{
  text    PT_LOAD    ;
  data    PT_LOAD    ;
  dynamic PT_DYNAMIC ;
}

SECTIONS
{
  . = SIZEOF_HEADERS;
  .text    : { *(.text*)   } : text    
  .rodata  : { *(.rodata*) } : text    
  .dynamic : { *(.dynamic) } : dynamic 
  .dynsym  : { *(.dynsym)  } : data    
  /DISCARD/ : {
    *(.eh_frame*)
    *(.gnu.hash*)
    *(.hash*)    
    *(.comment)  
    *(.symtab)   
    *(.strtab)   
  }
}

ENTRY (entrypoint)"#;

#[derive(Debug)]
pub struct BuildConfig {
    pub assembly_file: String,
    pub linker_file: Option<String>,
    pub debug: bool,
}

#[derive(Debug)]
pub struct BuildResult {
    pub object_file: String,
    pub shared_object_file: String,
    pub _temp_dir: TempDir, // Keep the temp directory alive
}

pub fn build_assembly(config: &BuildConfig) -> Result<BuildResult> {
    // Construct the path to the config file.
    let home_dir = home_dir().expect("Could not find $HOME directory");
    // Solana Config path.
    let config_path = home_dir.join(".config/solana/install/config.yml");

    if !Path::new(&config_path).exists() {
        return Err(Error::msg("Solana config not found. Please install the Solana CLI:\n\nhttps://docs.anza.xyz/cli/install"));
    }

    // Read the file contents
    let config_content = fs::read_to_string(config_path)?;

    // Parse the YAML file
    let solana_config: serde_yaml::Value = serde_yaml::from_str(&config_content)?;

    // Solana SDK and toolchain paths
    let active_release_dir = solana_config["active_release_dir"]
        .as_str()
        .ok_or_else(|| Error::msg("Could not find active_release_dir in Solana config"))?;

    let platform_tools = format!(
        "{}/bin/platform-tools-sdk/sbf/dependencies/platform-tools",
        active_release_dir
    );
    let llvm_dir = format!("{}/llvm", platform_tools);
    let clang = format!("{}/bin/clang", llvm_dir);
    let ld = format!("{}/bin/ld.lld", llvm_dir);

    // Check for platform tools
    if !Path::new(&llvm_dir).exists() {
        return Err(Error::msg(format!("Solana platform-tools not found. Please download the latest release from here: https://docs.solanalabs.com/cli/install")));
    }

    // Create temporary directory for build artifacts.
    let temp_dir = TempDir::new()?;
    let dbg_dir = temp_dir.path().to_string_lossy().to_string();

    // Extract filename without extension from assembly file path.
    let assembly_path = Path::new(&config.assembly_file);
    let filename = assembly_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::msg("Invalid assembly file path"))?;

    // Generate object file path.
    let object_file = format!("{}/{}.o", dbg_dir, filename);

    // Compile assembly to object file.
    compile_assembly(&clang, &config.assembly_file, &object_file, config.debug)?;

    // Handle linker file.
    let linker_file = if let Some(ref custom_linker) = config.linker_file {
        custom_linker.clone()
    } else {
        // Generate default linker file
        let default_linker = format!("{}/linker.ld", dbg_dir);
        fs::write(&default_linker, DEFAULT_LINKER)?;
        default_linker
    };

    // Generate shared object file path.
    let shared_object_file = format!("{}/{}.so", dbg_dir, filename);

    // Build shared object.
    build_shared_object(&ld, &object_file, &linker_file, &shared_object_file)?;

    Ok(BuildResult {
        object_file,
        shared_object_file,
        _temp_dir: temp_dir,
    })
}

fn compile_assembly(clang: &str, input_file: &str, output_file: &str, debug: bool) -> Result<()> {
    let mut clang_args = vec!["-target", "sbf", "-c", "-o", output_file, input_file];

    if debug {
        clang_args.push("-g");
    }

    let status = Command::new(clang).args(clang_args).status()?;

    if !status.success() {
        eprintln!("Failed to compile assembly file: {}", input_file);
        return Err(Error::new(io::Error::new(
            io::ErrorKind::Other,
            "Compilation failed",
        )));
    }

    Ok(())
}

fn build_shared_object(
    ld: &str,
    input_file: &str,
    linker_file: &str,
    output_file: &str,
) -> Result<()> {
    let status = Command::new(ld)
        .arg("-shared")
        .arg("-z")
        .arg("notext")
        .arg("--image-base")
        .arg("0x100000000")
        .arg("-T")
        .arg(linker_file)
        .arg("-o")
        .arg(output_file)
        .arg(input_file)
        .status()?;

    if !status.success() {
        eprintln!("Failed to build shared object: {}", output_file);
        return Err(Error::new(io::Error::new(
            io::ErrorKind::Other,
            "Linking failed",
        )));
    }

    Ok(())
}
