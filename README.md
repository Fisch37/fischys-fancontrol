## Prerequisites
- Linux
- The [rust toolchain](https://rust-lang.org/tools/install/)
- `clang`
    - (if on Ubuntu, use `apt install clang`)
- (optional) SystemD

## Installation
1. Make sure the following packages are installed:
    - `libsensors-dev`
    - `lm-sensors`
    - (feature nvml) nvidia proprietary drivers
        or some other thing that provides `libnvidia-ml.so.1`,
        what do I know
2. Go through `sensors-detect` if you haven't before and ensure the relevant kernel modules are loaded
3. Run
    ```bash
    cargo build -r
    sudo cp ./target/release/fischys-fancontrol /usr/bin/fischys-fancontrol
    ```
    - Note: If you want to use the NVML feature, run `cargo build -r -F nvml` as the first line.
4. If you want to install the systemd service, also run
    ```bash
    sudo cp fischys-fancontrol.service /etc/systemd/system/
    ```

## Configuring the Service
_fischys-fancontrol works out of the box. Despite its existence and convenience, you do not need to run any commands before starting the service._

An example configuration is shown in [fan-curves.json](fan-curves.json).
Given that this is the configuration for my machine, it probably won't work out for you.
You should use the `fischys-fancontrol list-sensors` command to get an overview of your sensors and _TODO: We need a tool like list-sensors, but for pwms_. After that you can build a `fan-curves.json` to suit your needs.