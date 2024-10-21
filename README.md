# nRF BLE firmware update utility

Firmware update utility for BLE devices that support the
[nRF DFU](https://infocenter.nordicsemi.com/topic/sdk_nrf5_v17.1.0/lib_dfu_transport_ble.html) protocol.

An alternative to the official  [nrfutil](https://infocenter.nordicsemi.com/topic/ug_nrfutil/UG/nrfutil/nrfutil_dfu_ble.html)
which needs a special USB device connected to the host machine to run a BLE update.

## Usage

Typical use:
- `nrfdfu-ble -a 01:23:45:67:89:AB -p ./my_dfu.zip`
- `nrfdfu-ble -n "Dev_w_BtnlessDFU" -p ./my_dfu.zip`
- `nrfdfu-ble -n "DfuTarg" -p ./my_dfu.zip`

> NOTE!
>
> Be careful with the last example as `DfuTarg` is the default 
> BLE device name when Nordic DFU projects are in DFU mode. 
> 
> It's easy to attempt DFU on an unintended device if multiple
> BLE devices are in DFU mode in the same physical setting at the same time.

Running `nrfdfu-ble --help` should show
```console
Update firmware on nRF BLE DFU targets

Usage: nrfdfu-ble.exe [OPTIONS]

Options:
  -n, --name <NAME>  BLE DFU target name [default: ]
  -a, --addr <ADDR>  BLE Address [default: ]
  -p, --pkg <PKG>    Firmware update package path [default: ]
  -h, --help         Print help
  -V, --version      Print version
```

