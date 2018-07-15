# loudspin

[![Build Status](https://travis-ci.org/dmnsafonov/loudspin.svg?branch=master)](https://travis-ci.org/dmnsafonov/loudspin)

Silence your hdds by one non-root command (a thin wrapper around `hdparm -M`).

Works as simple as `loudspin quiet` and `loudspin loud`.

Check your settings with
```console
$ loudspin

/dev/sdb:
 acoustic      = 254 (128=quiet ... 254=fast)

/dev/sdc:
 acoustic      = 254 (128=quiet ... 254=fast)
```

Setup the devices in /etc/loudspin.conf:
```toml
hdparm_path = "/sbin/hdparm" # default, can be omitted
devices = ["/dev/sd[bc]", "/dev/custom_device_*"] # glob patterns

[levels]
# default "loud = 254" and "quiet = 128" levels are always present,
# but can be overridden
loud = 253

# you can define your own AAM levels
medium = 196
noisy = 234
```

# Installation

For loudspin use by non-root user, you need to set cap_dac_override
and cap_sys_rawio file capabilities on the executable.

The overall process is like that:
```console
$ git clone https://github.com/dmnsafonov/loudspin.git
$ cd loudspin
$ cargo build --release
$ su
# cp target/release/loudspin /usr/local/bin/
# setcap 'cap_dac_override,cap_sys_rawio=p' /usr/local/bin/loudspin
# exit
$ echo "$PATH:/usr/local/bin" >> ~/.bashrc
```
