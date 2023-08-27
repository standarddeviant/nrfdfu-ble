# nRF BLE firmware update utility

Firmware update utility for BLE devices that support the
[nRF DFU](https://infocenter.nordicsemi.com/topic/sdk_nrf5_v17.1.0/lib_dfu_transport_ble.html) protocol.

An alternative to the official  [nrfutil](https://infocenter.nordicsemi.com/topic/ug_nrfutil/UG/nrfutil/nrfutil_dfu_ble.html)
which needs a special USB device connected to the host machine to run a BLE update.

## Usage

```console
nrfdfu-ble DfuTargetName /path/to/fw-pkg.zip
```
