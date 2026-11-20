# Legacy2UEFI — premier livrable P0 (mandat v1.1, §43)

Date : 2026-11-19 · Auteur : Claude Code · Statut : **PROPOSITION — rien n'est implémenté**

> **Révision 2026-11-19 (décision du propriétaire) :** la machine de référence active est
> l'**Alienware Aurora R1**. Le Dell OptiPlex 980 est **différé**, pas abandonné. Partout où ce
> document dit « Dell » ou « golden », lire « machine de référence (Aurora R1) ». En particulier :
> - critère GO n° 2 → une config qui démarre un OS sur l'Aurora R1 est dans `profiles/`, avec la version OpenCore exacte ;
> - périmètre P1 → x86-64, Aurora R1 seulement ;
> - §13 → remplacé par la « Next safe action » de `agent.md`.
> Résultats terrain déjà obtenus : expérience E1 (`docs/experiments/E1-openduet-smoke-test.md`).

Ce que ce document peut et ne peut pas affirmer :

- **Vérifié ici** : skills installés (avec leurs commits), contenu de l'archive `OpenCore-1.0.7-RELEASE.zip` présente sur le poste (lue sans extraction), comportement du script upstream `BootInstallBase.sh`.
- **Non disponible ici** : le mandat initial v1.0, la configuration Dell OptiPlex 980 connue fonctionnelle, la version d'OpenCore réellement utilisée sur le Dell. Tout ce qui les concerne est marqué **À IMPORTER**.
- **Non vérifié** : tout le comportement matériel. Aucun test virtuel ou physique n'a été exécuté. Ce poste n'a ni Rust, ni QEMU, ni WSL.

---

## 1. Inventaire des skills

| Skill | Présent | Provenance / commit |
|---|---|---|
| rust-best-practices | oui (utilisateur, `~/.claude/skills`) | apollographql/skills @ `9dda139` (v1.1.2) |
| security-threat-model | oui | openai/skills @ `49f948f` |
| uml-and-software-architecture-visualization | oui | openai/plugins @ `981b02c` |
| legacy2uefi-domain / -hardware / -testing / -ui | **créés en DRAFT** dans `.skills/` | ce dépôt |

Détail complet : [SKILLS.md](SKILLS.md).

## 2. Skills manquants

- **Planification (§35.1 D)** : aucun skill dédié installé. Je propose de ne pas en ajouter : le
  workflow en 7 étapes du mandat, consigné dans l'ADR ou la PR, plus le plan mode de Claude Code,
  suffit. À trancher par toi.
- **Exposition à Claude Code** : les skills projet sont dans `.skills/`, mais Claude Code ne lit que
  `.claude/skills/`. Il faudra un lien ou une copie (décision mineure, à faire au moment de créer le dépôt réel).
- **Reproductibilité** : les skills externes sont installés par utilisateur et non versionnés. Un
  autre poste ou un autre agent doit les réinstaller aux mêmes commits.

## 3. Plan de création des quatre skills Legacy2UEFI

Étape 1, faite : brouillons v0.1 rédigés à partir de l'addendum.
1. **domain** : définition, vocabulaire, architecture, invariants, niveaux, politique OpenCore. C'est la source principale ; les trois autres y renvoient au lieu de répéter.
2. **hardware** : méthodes de détection et règle *detect capabilities, not brands*.
3. **testing** : transitions d'état, preuves physiques, QEMU, régression golden.
4. **ui** : chaîne de repli GUI → TUI → texte, couleurs, clavier.

Reste à faire avant P1 (§35.4) : (a) les relire avec le mandat v1.0 et la config Dell réelle,
(b) corriger les contradictions, (c) passer de DRAFT à 1.0, (d) remplir les `references/` (doc
OpenCore épinglée, rapports bruts du Dell).

## 4. Proposition de `agent.md`

Rédigé : [../agent.md](../agent.md). `AGENTS.md` et `CLAUDE.md` ne font que pointer vers lui, ce qui
évite toute divergence. Les clés lues par la CI (`PROJECT_PHASE`, `OPENCORE_VERSION`,
`REQUIRED_SKILLS`) sont contrôlées par `tools/check-agent-md.sh`.

## 5. Audit de la stack

| Brique | Verdict | Points d'attention |
|---|---|---|
| Rust | **Valider** | Binaires statiques musl. Parsers de disques non fiables : c'est là que Rust apporte le plus. Garder la porte `no_std`/UEFI (crate `uefi`) pour l'option C. |
| POSIX shell | **Valider** | Limité à la colle : hooks initramfs, assemblage d'image, CI. Aucune écriture disque en shell : toute écriture passe par le module Rust unique. |
| Ratatui + Crossterm | **Valider** | Fonctionne sur la console VT Linux sans DRM. Tester en 16 couleurs et en monochrome. |
| Slint LinuxKMS + rendu logiciel | **Valider sous réserve** | À mesurer : taille des dépendances dans l'initramfs (libinput, udev, xkbcommon), repli `/dev/fb*` réellement disponible dans la version épinglée, fluidité 1024×768 sur un CPU Nehalem, licence (GPLv3 / libre de redevances / commerciale) compatible avec celle du projet, qui n'est pas encore choisie. Si l'un de ces points échoue, la GUI reste en P3 : elle n'est pas bloquante. |
| Python | Dev seulement | Absent de ce poste, sans conséquence. |
| IA32 (CPU 32 bits seulement) | **Hors P1** | OpenDuet et OpenCore existent en IA32, mais cela double la matrice de test. |

Pas de changement de stack proposé.

## 6. Audit OpenDuet / OpenCore (sur l'archive 1.0.7 présente)

| Composant | Constat |
|---|---|
| **LegacyBoot / OpenDuet** | `boot0` (MBR, 512 octets), `boot1f32` (PBR FAT32), `bootX64` / `bootIA32` (l'environnement UEFI, copié en `/boot` à la racine de la partition FAT32), variantes **`-blockio`** (accès disque via INT 13h du BIOS, piste intéressante pour les contrôleurs sans pilote UEFI). Le README indique une cible **GPT** ; le script gère aussi le MBR (bit actif). |
| **BootInstall** | `BootInstallBase.sh` demande un nom de disque **tapé à la main**, `dd` de `boot0` sur le MBR de ce disque, réécrit le secteur de boot de la partition et peut basculer le bit actif via `fdisk`. Il ne garde aucune sauvegarde (`origbs` est supprimé au lancement suivant). **Inutilisable tel quel dans le produit** (menace T1). On reproduit ses trois écritures en Rust, sur image ou sur clé identifiée uniquement (ADR-0003). |
| **QemuBuild.command** | Référence pour construire une image de test QEMU. Depuis 1.0.6, il gère aussi un mode EFI sans Duet. |
| **OpenLinuxBoot** | Pilote fourni (X64 et IA32). Permet de démarrer un noyau Linux directement. Utile pour certaines distributions, mais Bazzite, à base Fedora Atomic, passe normalement par shim/GRUB. À évaluer avec la config golden. |
| **OpenLegacyBoot** | Pilote fourni. Démarre un OS *legacy* depuis OpenCore. Intéressant comme entrée « revenir à l'OS d'origine » sans retirer la clé. |
| **Pilotes utiles** | `XhciDxe`, `NvmExpressDxe`, `OpenUsbKbDxe`, `Ps2KeyboardDxe`, `Ps2MouseDxe`, `OpenPartitionDxe`, `Ext4Dxe`. |
| **ocvalidate** | Binaires Linux, macOS et Windows fournis. À exécuter en CI sur chaque config générée, avec la même version qu'OpenCore. |
| **1.0.7** | Changelog : base edk2-stable202511, changements surtout orientés macOS. **Rien n'indique que c'est la version du Dell** : elle reste UNCONFIRMED. |

## 7. Proposition bootstrap

Détail : [adr/0002-bootstrap-architecture.md](adr/0002-bootstrap-architecture.md), diagrammes dans [BOOTFLOW.md](BOOTFLOW.md).

- **A seul** (OpenDuet direct) : c'est ce qui marche déjà sur le Dell. Mais sans diagnostic ni TUI.
- **B tel qu'écrit** (Linux puis OpenDuet dans le même démarrage) : **probablement irréalisable**. Une fois Linux lancé, on ne peut plus revenir aux services BIOS en mode réel dont OpenDuet dépend.
- **Proposition : B2, deux démarrages.** Un menu sur la clé propose le pont par défaut (dernier profil
  validé) ou le Linux de diagnostic. Le Linux écrit `config.plist` **sur la clé**, puis on redémarre.
  Le Linux de diagnostic sert aussi de recovery quand le pont échoue.
- **C, piste P3+** : une sonde Rust `no_std` lancée par OpenCore, pour décider dans le même démarrage.

Expérience P0 nécessaire : multiplexer les deux entrées (SYSLINUX ou GRUB i386-pc devant `boot1f32`,
ou Linux lancé depuis OpenCore). À tester en QEMU + SeaBIOS.

## 8. Structure du dépôt

Celle du §38 est retenue, avec trois ajustements :

- `crates/image-writer/` séparé de `recovery/` : **un seul** crate a le droit d'écrire sur un périphérique bloc. C'est ce qui rend l'invariant auditable.
- `tests/evidence/` : preuves physiques par modèle et par date (exigées pour VERIFIED).
- `docs/MANDATE.md` : le mandat lui-même, versionné (« le dépôt fait foi »).

Squelette créé dans ce dossier : `agent.md`, `AGENTS.md`, `CLAUDE.md`, `README.md`, `.skills/` (4), `docs/` (SKILLS, P0-AUDIT, SECURITY, BOOTFLOW, adr/0000–0005), `third_party/opencore/version`, `tools/check-agent-md.sh`, et des répertoires vides pour le reste.

## 9. Threat model initial

[SECURITY.md](SECURITY.md) : 10 menaces. Les deux critiques :
**T1** écriture sur le mauvais périphérique, et **T2** ouverture en écriture d'un disque interne par
bug (montage sans `ro`, rejeu de journal ext4, fsck automatique). Trois questions ouvertes à la fin
du document te sont adressées.

## 10. Risques majeurs

| # | Risque | Conséquence | Réponse |
|---|---|---|---|
| R1 | Config golden perdue ou non reproductible | P1 sans référence | L'importer en premier, en lecture seule |
| R2 | Multiplexage pont / diagnostic impossible proprement | B2 dégradé | Expérience QEMU en P0 ; repli : deux clés ou A seul + diagnostic UEFI (C) |
| R3 | OpenDuet instable sur certains chipsets ou BIOS (carte mémoire, USB) | Matrice de compatibilité étroite | Statuts honnêtes, variantes blockio, base de compatibilité |
| R4 | Taille et complexité de l'initramfs avec Slint | Démarrage lent, maintenance | GUI optionnelle, mesurée avant d'être acceptée |
| R5 | Écriture sur un disque interne | Perte de données | ADR-0005, crate d'écriture unique, hashes QEMU |
| R6 | Évolutions OpenCore orientées macOS qui cassent des cas Linux/Windows | Régressions | Version épinglée, golden test + ocvalidate à chaque mise à jour |
| R7 | Surpromesse (« transforme le BIOS en UEFI », VERIFIED trop tôt) | Confiance perdue | Vocabulaire du skill domain, preuves obligatoires |

## 11. Périmètre réaliste P0 / P1

**P0 (architecture et faisabilité, sans code produit)**

- Gouvernance : fait en brouillon.
- Import de la golden reference et du mandat v1.0.
- Expériences QEMU + SeaBIOS : (1) OpenDuet 1.0.7 + OpenCore depuis une image USB, puis UEFI Shell ; (2) multiplexage des deux entrées ; (3) hash des disques internes avant et après.
- Mesure Slint (taille, framebuffer, vitesse) : un prototype jetable est autorisé, hors du chemin critique.
- ADR 0001–0004 passés en Accepted ou réécrits.

**P1 (golden reference reproduite par le produit)**

- x86-64 seulement, Dell OptiPlex 980 seulement.
- Crates : `hardware-probe` (lecture seule), `opencore-config` (régénère la config golden à l'identique sémantique), `image-writer` (image fichier d'abord), `logging`, TUI minimale.
- Aucun autre matériel promu au-delà d'EXPERIMENTAL.

Hors P1 : GUI Slint, IA32, profils multiples, OpenLegacyBoot, sonde UEFI.

## 12. Critères GO / NO-GO

**Sortie de P0 → GO P1 si tout est vrai :**

1. Les quatre skills sont en v1.0 et relus contre le mandat v1.0, sans contradiction ouverte.
2. La config golden du Dell est dans `profiles/golden/` avec la version OpenCore exacte, et `OPENCORE_VERSION` n'est plus UNCONFIRMED.
3. En QEMU + SeaBIOS, une image USB construite par script atteint le picker OpenCore puis le UEFI Shell, et les images de disques internes ont un hash inchangé.
4. ADR-0002 accepté, avec un mécanisme de multiplexage démontré en QEMU.
5. `tools/check-agent-md.sh` passe en CI.

**NO-GO, ou retour à l'architecture, si :**

- le multiplexage n'est pas démontrable → décider entre A seul, deux clés ou l'option C ;
- la config golden n'est pas récupérable → reconstruire une référence en EXPERIMENTAL avant toute chose ;
- un chemin quelconque exige d'écrire sur un disque interne.

**Sortie de P1 :** l'image produite par Legacy2UEFI démarre physiquement Bazzite (ou l'OS de
référence) sur le Dell, preuves dans `tests/evidence/`, config générée sémantiquement égale à la golden, `ocvalidate` vert.

## 13. Première action concrète sans risque

**Importer en lecture seule la référence Dell.** Sur la machine Linux où se trouve la clé fonctionnelle :

```bash
lsblk -o NAME,SIZE,MODEL,TRAN,LABEL,PARTUUID,FSTYPE
```

Identifier la clé (TRAN=usb), la monter en `ro`, puis copier `EFI/`, le fichier `/boot` à la racine et tout
log OpenCore dans `profiles/golden/dell-optiplex-980/`. Copier ensuite le mandat v1.0 dans `docs/MANDATE.md`.
Aucune écriture sur la clé ni sur un disque.

Puis : relecture des skills contre ces deux sources.

**STOP.** Pas de P1 avant que les critères GO ci-dessus soient remplis.
