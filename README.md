# pip-license-check

This is a Rust tool to check the licenses of Python packages installed via a pip requirements file.

## Usage

By default, `pip-license-check` will read from `requirements.txt` and `licenses.toml` in the current directory.

Options:

- `-r, --requirements <FILE>` - Path to requirements.txt file
- `-s, --settings <FILE>` - Path to TOML file with license settings 
- `-v, --verbose` - Print more info

## License Settings

The license settings file is in [TOML format](https://github.com/toml-lang/toml).

- `allowed` - List of allowed license names or regex patterns
- `disallowed` - List of disallowed license names or regex patterns 
- `ignored` - List of ignored package names
- `missing` - Mapping of package names to expected license

## Installation

cargo install pip-license-check

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
