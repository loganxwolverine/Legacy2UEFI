# Boot flow and decision diagrams

Mermaid, versioned with the code. Status: **PROPOSED** (ADR-0002). Nothing here is proven yet
beyond what the golden reference demonstrated outside this repo.

## 1. Boot chain (bridge path)

```mermaid
flowchart LR
  BIOS[BIOS Legacy<br/>real mode, INT 13h] --> MBR[USB MBR<br/>boot0]
  MBR --> PBR[FAT32 PBR<br/>boot1f32]
  PBR --> DUET[/boot<br/>OpenDuet = UEFI env/]
  DUET --> OC[EFI/BOOT/BOOTx64.efi<br/>OpenCore]
  OC --> LDR[OS EFI loader<br/>shim / GRUB / bootmgfw]
  LDR --> OS[Operating system]
  OC -.-> SHELL[UEFI Shell]
```

## 2. Two-stage architecture

```mermaid
flowchart TB
  BIOS[BIOS boots USB key] --> MENU{Key boot menu}
  MENU -- default --> BRIDGE[Bridge path<br/>OpenDuet → OpenCore<br/>last known-good profile]
  MENU -- Diagnostics --> LNX[Minimal Linux, BIOS mode]
  LNX --> PROBE[hardware-probe<br/>read-only]
  PROBE --> ENG[compatibility-engine]
  ENG --> PROF[profile-engine]
  PROF --> UI{GUI starts?}
  UI -- yes --> GUI[Slint GUI]
  UI -- no --> TUI[Ratatui TUI]
  GUI --> CHOICE[User confirms profile]
  TUI --> CHOICE
  CHOICE --> WRITE[Atomic write of config.plist<br/>ON THE KEY ONLY + backup]
  WRITE --> REBOOT[Reboot] --> BIOS
```

## 3. Profile selection / hardware decision tree

```mermaid
flowchart TB
  S[Probe report] --> CPU{x86-64 long mode?}
  CPU -- no --> UNS1[UNSUPPORTED in P1<br/>IA32 path = later]
  CPU -- unknown --> GREY1[UNKNOWN - grey]
  CPU -- yes --> USB{Key behind EHCI / xHCI / UHCI-OHCI?}
  USB --> STO{Target OS disk controller<br/>reachable from the bridge?}
  STO -- AHCI / IDE / NVMe with driver --> DB{Model in compatibility DB?}
  STO -- RAID / unknown --> BLK[EXPERIMENTAL candidate:<br/>blockio variant]
  STO -- not readable --> GREY2[UNKNOWN - grey]
  DB -- VERIFIED --> G[GREEN profile]
  DB -- no entry --> A[AMBER generic profile]
  DB -- FAILED --> R[RED + reason]
```

## 4. Recovery flow

```mermaid
flowchart TB
  F[Bridge fails or hangs] --> M[Reboot, pick Diagnostics]
  M --> L[Linux reads last OpenCore log from key]
  L --> SAFE{Safe mode}
  SAFE --> RB[Restore previous config.plist backup]
  SAFE --> MIN[Write minimal safe profile]
  SAFE --> SB[Export support bundle to key]
  RB --> R2[Reboot]
  MIN --> R2
```

## 5. Failure states

```mermaid
stateDiagram-v2
  [*] --> BiosBoot
  BiosBoot --> DuetLoaded: boot0/boot1f32 OK
  BiosBoot --> F_NoBoot: key not bootable / BIOS refuses USB-HDD
  DuetLoaded --> OpenCoreRunning
  DuetLoaded --> F_DuetHang: OpenDuet hang (memory map, USB, storage)
  OpenCoreRunning --> PickerShown
  OpenCoreRunning --> F_ConfigInvalid: config.plist invalid
  PickerShown --> OsLoader
  PickerShown --> F_NoEntries: OS disk not visible (driver/controller)
  OsLoader --> OsRunning
  OsLoader --> F_LoaderFail: loader error (GOP, memory, Secure Boot expectations)
  F_NoBoot --> Diagnostics
  F_DuetHang --> Diagnostics
  F_ConfigInvalid --> Diagnostics
  F_NoEntries --> Diagnostics
  F_LoaderFail --> Diagnostics
  OsRunning --> [*]
```

## 6. Data flow and trust

```mermaid
flowchart LR
  HW[(Hardware: CPUID, SMBIOS, PCI)] -->|read| P[probe]
  DISK[(Internal disks)] -->|read-only| P
  P --> R[report.json<br/>privacy-filtered]
  R --> E[engine] --> C[config.plist]
  DB[(compat DB + profiles<br/>in repo)] --> E
  C -->|write| KEY[(USB key ESP)]
  R -->|write| KEY
  LOG[logs] -->|write| KEY
  KEY -->|export| BUNDLE[support bundle]
```
