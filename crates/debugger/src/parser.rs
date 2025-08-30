use gimli::{EndianSlice, RunTimeEndian, SectionId};
use object::{Object, ObjectSection, ObjectSymbol};
use solana_sbpf::ebpf::MM_RODATA_START;
use solana_sbpf::elf_parser::Elf64;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::error::DebuggerError;

#[derive(Debug, Clone)]
pub struct ROData {
    pub name: String,
    pub address: u64,
    pub content: String,
}

pub fn parse_rodata(file_path: &str, debug_file_path: &str) -> Result<Vec<ROData>, DebuggerError> {
    let file = fs::File::open(debug_file_path)?;
    let mmap = unsafe { memmap2::Mmap::map(&file).unwrap() };
    let object = object::File::parse(&*mmap).unwrap();

    // Find the .rodata section.
    let rodata_section = object.sections().find(|section| {
        section
            .name()
            .map(|name| name == ".rodata")
            .unwrap_or(false)
    });

    let rodata_section = match rodata_section {
        Some(section) => section,
        None => {
            println!("No .rodata section found");
            return Ok(vec![]);
        }
    };

    let rodata_addr = rodata_section.address();
    let rodata_data = rodata_section.uncompressed_data()?;
    let section_end = rodata_addr + rodata_data.len() as u64;

    // Get all .rodata symbols sorted by address.
    let mut symbols: Vec<_> = object
        .symbols()
        .filter_map(|symbol| {
            if let Some(index) = symbol.section_index() {
                if index == rodata_section.index() {
                    Some((
                        symbol.address(),
                        symbol.name().unwrap_or("<unnamed>").to_string(),
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    symbols.sort_by_key(|(addr, _)| *addr);

    // Extract the actual rodata offset from the .so file.
    let mut rodata_offset = 0;
    let file_data = std::fs::read(file_path)?;
    let elf = Elf64::parse(&file_data).unwrap();
    // Find the .rodata section.
    for section_header in elf.section_header_table() {
        let section_name = elf.section_name(section_header.sh_name).unwrap();
        if section_name == b".rodata" {
            rodata_offset = section_header.sh_addr;
        }
    }

    // Extract data for each symbol.
    let mut results = Vec::new();
    for (i, (addr, name)) in symbols.iter().enumerate() {
        let offset = if rodata_addr == 0 {
            *addr as usize
        } else {
            (*addr - rodata_addr) as usize
        };
        // Determine end of this symbol's data.
        let next_addr = if i + 1 < symbols.len() {
            symbols[i + 1].0
        } else {
            section_end
        };
        let size = (next_addr - *addr) as usize;
        let content = if offset < rodata_data.len() {
            let end = std::cmp::min(offset + size, rodata_data.len());
            &rodata_data[offset..end]
        } else {
            &[]
        };

        // Format as ASCII if printable else as hex.
        let msg = if content.iter().all(|&b| b.is_ascii_graphic() || b == b' ') {
            String::from_utf8_lossy(content).to_string()
        } else {
            content
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ")
        };

        let symbol_offset = *addr;
        let vm_address = MM_RODATA_START + rodata_offset + symbol_offset;
        results.push(ROData {
            name: name.clone(),
            address: vm_address,
            content: msg.clone(),
        });
    }

    Ok(results)
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
    _address: u64,
}

pub struct LineMap {
    /// Maps instruction addresses to source line numbers
    address_to_line: HashMap<u64, usize>,
    /// Maps line numbers to instruction addresses
    line_to_addresses: HashMap<usize, Vec<u64>>,
    /// Maps DWARF addresses to actual SBPF program counters
    dwarf_to_pc: HashMap<u64, u64>,
    /// Maps SBPF program counters to DWARF addresses
    pc_to_dwarf: HashMap<u64, u64>,
    /// Complete source location information
    source_locations: HashMap<u64, SourceLocation>,
    /// Line to address mapping with file names
    line_to_address: HashMap<(String, u32), u64>,
    /// File names
    files: Vec<String>,
}

impl LineMap {
    pub fn new() -> Self {
        Self {
            address_to_line: HashMap::new(),
            line_to_addresses: HashMap::new(),
            dwarf_to_pc: HashMap::new(),
            pc_to_dwarf: HashMap::new(),
            source_locations: HashMap::new(),
            line_to_address: HashMap::new(),
            files: Vec::new(),
        }
    }

    /// Parse DWARF debug information from an ELF file
    pub fn from_elf_file(file_path: &str) -> Result<Self, DebuggerError> {
        let file_data = std::fs::read(file_path)?;
        Self::from_elf_data(&file_data)
    }

    /// Parse DWARF debug information from ELF data
    pub fn from_elf_data(file_data: &[u8]) -> Result<Self, DebuggerError> {
        let object = object::File::parse(file_data)?;

        let mut line_map = Self::new();

        // Parse DWARF debug information directly from the object
        line_map.parse_debug_info_from_object(&object)?;

        // Build the PC mapping after parsing
        line_map.build_pc_mapping();

        Ok(line_map)
    }

    /// Parse debug information and build line mapping from object file
    fn parse_debug_info_from_object(
        &mut self,
        obj_file: &object::File,
    ) -> Result<(), DebuggerError> {
        // Determine endianness
        let endian = if obj_file.is_little_endian() {
            RunTimeEndian::Little
        } else {
            RunTimeEndian::Big
        };

        // Load DWARF sections
        let load_section = |id: SectionId| -> Result<Cow<[u8]>, gimli::Error> {
            match obj_file.section_by_name(id.name()) {
                Some(section) => match section.uncompressed_data() {
                    Ok(data) => Ok(data),
                    Err(_) => Ok(Cow::Borrowed(&[])),
                },
                None => Ok(Cow::Borrowed(&[])),
            }
        };

        // Borrow a `Cow<[u8]>` to create an `EndianSlice`
        let borrow_section = |section| EndianSlice::new(Cow::as_ref(section), endian);

        // Load all of the sections
        let dwarf_sections =
            gimli::DwarfSections::load(&load_section).map_err(|e| DebuggerError::ReadError(e))?;

        // Create `EndianSlice`s for all of the sections
        let dwarf = dwarf_sections.borrow(borrow_section);

        // Iterate over the compilation units
        let mut iter = dwarf.units();
        while let Some(header) = iter.next().map_err(|e| DebuggerError::UnitError(e))? {
            let unit = dwarf
                .unit(header)
                .map_err(|e| DebuggerError::UnitError(e))?;
            let unit = unit.unit_ref(&dwarf);

            // Get the line program for the compilation unit
            if let Some(program) = unit.line_program.clone() {
                let comp_dir = if let Some(ref dir) = unit.comp_dir {
                    PathBuf::from(dir.to_string_lossy().into_owned())
                } else {
                    PathBuf::new()
                };

                // Iterate over the line program rows
                let mut rows = program.rows();
                while let Some((header, row)) =
                    rows.next_row().map_err(|e| DebuggerError::ReadError(e))?
                {
                    if !row.end_sequence() {
                        // Determine the file path
                        let mut file_path = String::new();
                        if let Some(file) = row.file(header) {
                            let mut path = PathBuf::new();
                            path.clone_from(&comp_dir);

                            // The directory index 0 is defined to correspond to the compilation unit directory
                            if file.directory_index() != 0 {
                                if let Some(dir) = file.directory(header) {
                                    path.push(
                                        unit.attr_string(dir)
                                            .map_err(|e| DebuggerError::ReadError(e))?
                                            .to_string_lossy()
                                            .as_ref(),
                                    );
                                }
                            }

                            path.push(
                                unit.attr_string(file.path_name())
                                    .map_err(|e| DebuggerError::ReadError(e))?
                                    .to_string_lossy()
                                    .as_ref(),
                            );
                            file_path = path.to_string_lossy().to_string();
                        }

                        // Determine line/column
                        let line = match row.line() {
                            Some(line) => line.get() as u32,
                            None => 0,
                        };
                        let column = match row.column() {
                            gimli::ColumnType::LeftEdge => 0,
                            gimli::ColumnType::Column(column) => column.get() as u32,
                        };

                        let address = row.address();

                        // Store the mapping
                        self.address_to_line.insert(address, line as usize);

                        // Add to line_to_addresses
                        self.line_to_addresses
                            .entry(line as usize)
                            .or_insert_with(Vec::new)
                            .push(address);

                        // Create source location
                        let source_loc = SourceLocation {
                            file: file_path.clone(),
                            line,
                            column,
                            _address: address,
                        };
                        self.source_locations.insert(address, source_loc);

                        // Add to line_to_address mapping
                        self.line_to_address
                            .insert((file_path.clone(), line), address);

                        // Add file to files list if not already present
                        if !file_path.is_empty() && !self.files.contains(&file_path) {
                            self.files.push(file_path);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl LineMap {
    /// Build mapping between DWARF addresses and SBPF program counters
    fn build_pc_mapping(&mut self) {
        // For SBPF, the DWARF addresses are typically the actual instruction addresses
        // We map them directly to program counters
        for &dwarf_addr in self.address_to_line.keys() {
            self.dwarf_to_pc.insert(dwarf_addr, dwarf_addr);
            self.pc_to_dwarf.insert(dwarf_addr, dwarf_addr);
        }
    }

    /// Get the source line number for a given instruction address
    pub fn get_line_for_address(&self, address: u64) -> Option<usize> {
        self.address_to_line.get(&address).copied()
    }

    /// Get all instruction addresses for a given line number
    pub fn get_addresses_for_line(&self, line: usize) -> Option<&[u64]> {
        self.line_to_addresses.get(&line).map(|v| v.as_slice())
    }

    /// Get the current line number for a PC (program counter)
    pub fn get_line_for_pc(&self, pc: u64) -> Option<usize> {
        // Try to find the DWARF address for this PC
        if let Some(&dwarf_addr) = self.pc_to_dwarf.get(&pc) {
            return self.get_line_for_address(dwarf_addr);
        }

        // If not found, try the PC directly as it might be the same as DWARF address
        self.get_line_for_address(pc)
    }

    /// Get all PCs for a given line number
    pub fn get_pcs_for_line(&self, line: usize) -> Vec<u64> {
        if let Some(dwarf_addresses) = self.get_addresses_for_line(line) {
            dwarf_addresses
                .iter()
                .filter_map(|&dwarf_addr| {
                    // Convert DWARF address to PC
                    self.dwarf_to_pc.get(&dwarf_addr).copied()
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get source location for a given address
    pub fn get_source_location(&self, address: u64) -> Option<&SourceLocation> {
        self.source_locations.get(&address)
    }

    /// Get debug information about the line mapping
    pub fn debug_info(&self) -> String {
        let mut info = String::new();
        info.push_str(&format!(
            "Total DWARF address mappings: {}\n",
            self.address_to_line.len()
        ));
        info.push_str(&format!(
            "Total line mappings: {}\n",
            self.line_to_addresses.len()
        ));
        info.push_str(&format!("Total PC mappings: {}\n", self.pc_to_dwarf.len()));
        info.push_str(&format!(
            "Total source locations: {}\n",
            self.source_locations.len()
        ));

        if !self.address_to_line.is_empty() {
            info.push_str("Sample DWARF address mappings:\n");
            let mut count = 0;
            for (dwarf_addr, line) in self.address_to_line.iter().take(5) {
                let pc = self.dwarf_to_pc.get(dwarf_addr).unwrap_or(&0);
                info.push_str(&format!(
                    "  DWARF 0x{:x} -> PC 0x{:x} -> line {}\n",
                    dwarf_addr, pc, line
                ));
                count += 1;
            }
            if count < self.address_to_line.len() {
                info.push_str(&format!(
                    "  ... and {} more\n",
                    self.address_to_line.len() - count
                ));
            }
        }

        info
    }

    pub fn get_line_to_addresses(&self) -> &std::collections::HashMap<usize, Vec<u64>> {
        &self.line_to_addresses
    }
}
