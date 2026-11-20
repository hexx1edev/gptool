# gptool

A minimal GPT disk image utility built for OS development.

Creates, partitions, and manipulates raw GPT disk images from the command line — no loopback mounts, no root, no fuss. Images are `fdisk`-compatible and spec-compliant (UEFI 2.10 GPT layout with protective MBR, dual headers, and CRC32 integrity).

## Build

```
cargo build --release
```

NASM is only needed if you want to modify the boot sector source (`boot/bootsect.asm`). The compiled bytes are already embedded in the binary.

## Usage

### Create an image

```
gptool create disk.img 256M
```

Sizes accept `K`, `M`, `G`, `T` suffixes (binary units). Values are automatically rounded up to the nearest 512-byte sector boundary.

### Add partitions

```
gptool mkpart disk.img 64M  efi       "EFI System"
gptool mkpart disk.img 128M linux     "rootfs"
gptool mkpart disk.img 32M  swap      "swap"
gptool mkpart disk.img 1M   bios      "BIOS Boot"
gptool mkpart disk.img 16M  microsoft "Windows Data"
```

Partitions are placed into the first available gap on disk and the first empty entry slot (up to 128).

### List partitions

```
$ gptool list disk.img

        Disk disk.img  256M  (GUID 7B81A9B7-A975-4613-8B19-896DF1608F0A)

    #      Type                         Size   Name
    ────────────────────────────────────────────────────────────
    1      EFI System                    64M   EFI System
    2      Linux Filesystem             128M   rootfs
    3      Linux Swap                    32M   swap
    4      BIOS Boot                      1M   BIOS Boot
    5      Microsoft Basic Data          16M   Windows Data
```

### Edit a partition

```
gptool setpart disk.img 1 --name="ESP" --size=128M --type=efi
```

All flags are optional. Resizing checks for overlaps with adjacent partitions.

### Remove a partition

```
gptool rmpart disk.img 3
```

The entry slot is zeroed and reclaimed by the next `mkpart`.

### Dump partition contents

```
gptool read disk.img 2 rootfs.img
```

### Write data into a partition

```
gptool write disk.img 1 esp.img
```

If the input file is larger than the partition, the data is cropped with a warning. If smaller, the remaining space is zero-filled.

## Partition types

| Name        | GUID                                   | Description            |
|-------------|----------------------------------------|------------------------|
| `efi`       | `C12A7328-F81F-11D2-BA4B-00A0C93EC93B` | EFI System Partition   |
| `microsoft` | `EBD0A0A2-B9E5-4433-87C0-68B6B72699C7` | Microsoft Basic Data   |
| `linux`     | `0FC63DAF-8483-4772-8E79-3D69D8477DE4` | Linux Filesystem       |
| `bios`      | `21686148-6449-6E6F-744E-656564454649` | BIOS Boot Partition    |
| `swap`      | `0657FD6D-A4AB-43C4-84E5-0933C84B4F4F` | Linux Swap             |

## Disk layout

```
LBA 0           Protective MBR (with boot sector)
LBA 1           Primary GPT Header
LBA 2–33        Primary Partition Entry Array (128 × 128 bytes)
LBA 34–N-34     Usable space
LBA N-33–N-2    Backup Partition Entry Array
LBA N-1         Backup GPT Header
```

Sector size is 512 bytes. The protective MBR contains a small boot program that prints `Created by gptool. This media is not CSM-bootable.` and halts.

## Typical OS-dev workflow

```bash
gptool create disk.img 64M
gptool mkpart disk.img 32M efi "EFI System"
gptool mkpart disk.img 16M linux "kernel"

# build your EFI app / kernel, then flash them in
gptool write disk.img 1 esp.img
gptool write disk.img 2 kernel.bin

qemu-system-x86_64 -drive file=disk.img,format=raw -bios /usr/share/ovmf/OVMF.fd
```

## License

Do whatever you want with it.
