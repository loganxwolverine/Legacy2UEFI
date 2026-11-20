#!/bin/sh
# Legacy2UEFI — read-only report of the UEFI environment as seen by a Linux live session
# booted through the bridge. Writes the report next to this script (on the L2U key).
# Reads only. Serial numbers, UUIDs and MAC addresses are not collected.
d=$(dirname "$(readlink -f "$0")")
out="$d/l2u-report-$(date -u +%Y%m%d-%H%M%S).txt"
{
  echo "== date (UTC)"; date -u
  echo "== kernel"; uname -r
  echo "== /sys/firmware/efi"; ls /sys/firmware/efi
  echo "== fw_platform_size"; cat /sys/firmware/efi/fw_platform_size
  echo "== systab"; cat /sys/firmware/efi/systab 2>/dev/null
  echo "== efivarfs mount"; grep efivarfs /proc/mounts
  echo "== efivars count"; ls /sys/firmware/efi/efivars 2>/dev/null | wc -l
  echo "== efibootmgr -v"; efibootmgr -v 2>&1
  echo "== kernel log (efi/smbios)"; sudo -n dmesg 2>/dev/null | grep -iE 'efi|smbios|dmi:' | head -80
  echo "== bios/system/board"
  sudo -n dmidecode -t bios -t system -t baseboard 2>&1 | grep -E '^\s+(Vendor|Version|Release Date|Manufacturer|Product Name):'
  echo "== memory devices"
  sudo -n dmidecode -t memory 2>&1 | grep -E '^\s+(Locator|Size|Type|Speed|Configured Memory Speed|Manufacturer|Part Number):'
  echo "== cpu"; grep -m1 'model name' /proc/cpuinfo; echo "logical cpus: $(nproc)"
  echo "== lspci -nn"; lspci -nn
  echo "== lsusb"; lsusb
  echo "== lsblk"; lsblk -o NAME,SIZE,TRAN,TYPE,FSTYPE,LABEL,PARTTYPENAME
} > "$out" 2>&1
sync
echo
echo "Rapport ecrit sur la cle : $out"
echo "Tu peux fermer cette fenetre."
