# Legacy2UEFI — accueil

Une clé USB universelle qui apporte un environnement UEFI logiciel (OpenDuet + OpenCore) aux PC qui
n'ont qu'un BIOS Legacy, pour installer puis démarrer des systèmes qui exigent l'UEFI, comme Bazzite.
Le firmware n'est jamais modifié.

> Ce coffre Obsidian est le dossier du projet lui-même. Rien n'est recopié ailleurs : ce que tu lis ici,
> c'est ce qui fait foi.

## Commencer ici

- [État du projet et prochaine étape (agent.md)](agent.md) — la source de vérité, mise à jour à chaque étape
- [Journal](journal/) — ce qui s'est passé, jour par jour, avec les captures
- [Audit P0](docs/P0-AUDIT.md) — le premier livrable, architecture et plan

## Expériences

- [E1 — OpenDuet + OpenCore jusqu'au UEFI Shell](docs/experiments/E1-openduet-smoke-test.md) — réussie sur l'Alienware Aurora R1
- [E2 — démarrer l'installateur de Bazzite via la passerelle](docs/experiments/E2-bazzite-installer-boot.md) — réussie : Bazzite Live en mode UEFI
- [E3 — installer Bazzite puis le démarrer via la clé](docs/experiments/E3-bazzite-install-and-boot.md) — réussie, machine VERIFIED
- [E4 — Omarchy sur le ThinkPad avec l'image alpha.2](docs/experiments/E4-omarchy-thinkpad.md) — réussie, machine VERIFIED

## Décisions (ADR)

- [Liste des décisions](docs/adr/README.md)
- [0005 — aucune écriture sur les disques internes](docs/adr/0005-no-internal-disk-write.md)
- [0006 — clavier USB par défaut](docs/adr/0006-usb-keyboard-default.md)
- [0007 — la clé reste branchée après l'installation](docs/adr/0007-key-stays-plugged.md)
- [0008 — écran de démarrage graphique](docs/adr/0008-graphical-preos-launcher.md)

## Documentation

- [Schémas de démarrage](docs/BOOTFLOW.md)
- [Modèle de menaces](docs/SECURITY.md)
- [Skills du projet](docs/SKILLS.md)
- [Notes de version 0.1.0-alpha.3](docs/releases/0.1.0-alpha.3.md)
- [Licence GPL-3.0-or-later](LICENSE) et [composants tiers](THIRD-PARTY-NOTICES.md)

## Machines

| Machine | Statut | Preuves |
|---|---|---|
| Alienware Aurora R1 | **VERIFIED** — Bazzite installé et démarré via la clé (E3) | `tests/evidence/alienware-aurora-r1/` |
| Lenovo ThinkPad 4414WBF | **VERIFIED** — Omarchy installé et démarré via l'image alpha.2 (E4) | `tests/evidence/lenovo-thinkpad-4414wbf/` |
| Dell OptiPlex 980 | différé | — |
