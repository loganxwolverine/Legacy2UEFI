# l2u-key-gui — gestionnaire de clé USB Legacy2UEFI

Petit utilitaire Rust + Qt6 qui **crée** la clé USB Legacy2UEFI à partir d'une image de
release (`.img`) et **met à jour** une clé existante en place, sans réécrire le MBR ni le
secteur de démarrage de la partition.

Le cœur (lecture d'image, comparaison, écriture FAT32, vérification) est testable sans
interface ; l'interface Qt6 n'est qu'un client. Un mode CLI permet de tout faire sans
serveur graphique.

## Ce que fait l'outil

| Action | Effet |
|---|---|
| **Créer la clé** | Écrit l'image de release brute (MBR + FAT32) sur la cible, puis relit et vérifie tout (MBR, signature du secteur de démarrage, 16 fichiers). |
| **Analyser la clé** | Lit la clé et affiche version, build, identité Legacy2UEFI, nombre de fichiers. |
| **Mettre à jour la clé** | Compare l'image de release à la clé (version + contenu) et remplace **uniquement** l'arborescence `EFI/` : `EFI.NEW` → relecture → permutation `EFI` → `EFI.OLD`, `EFI.NEW` → `EFI`. Le MBR et le PBR restent intacts. |
| **Vérifier le dépôt** | Interroge les releases GitHub publiées et signale la plus récente. |

La mise à jour conserve les journaux `EFI.OLD` / `EFI.NEW` en cas d'interruption : une
nouvelle exécution reprend et nettoie l'état intermédiaire.

## Sécurité

- Refus du disque qui porte le système en cours d'exécution, des cibles non amovibles et
  des cibles trop petites pour FAT32.
- La mise à jour exige que la cible porte l'identité Legacy2UEFI (`LEGACY2UEFI-KEY.txt`) ;
  une clé quelconque est refusée.
- L'image de release est vérifiée contre `SHA256SUMS.txt` quand il est présent ; une image
  non conforme est refusée à l'écriture.
- Aucune écriture sur disque interne : la cible est ouverte en écriture uniquement après
  les contrôles ci-dessus, et jamais par nom de périphérique seul.

## Utilisation

```sh
# Interface graphique
cargo run --release

# Ligne de commande
cargo run --release -- --list-devices
cargo run --release -- --inspect /dev/sdX
cargo run --release -- --create chemin/legacy2uefi-0.1.0-alpha.4.img /dev/sdX
cargo run --release -- --update chemin/legacy2uefi-0.1.0-alpha.4.img /dev/sdX
cargo run --release -- --check-updates
```

Les opérations sur un **disque** demandent root (l'interface relance via `pkexec`). Une
cible qui est un **fichier** (image disque) ne demande aucun privilège : utile pour les
tests.

## Tests

```sh
cargo test          # 14 tests unitaires (moteur, comparaison, périphériques)
cargo clippy -- -D warnings
cargo fmt --check
```

Le parcours complet (création puis mise à jour) a été validé sur une image disque de
256 Mio et sur un périphérique bloc de test (`/dev/loop0`) dans une VM Solus :
création OK, mise à jour alpha.3 → alpha.4 OK, MBR identique octet pour octet.

## Limites connues

- **Jamais testé sur une machine BIOS Legacy physique** : uniquement en VM et sur
  périphérique bloc simulé. Le démarrage réel de la clé créée n'a pas été vérifié.
- Le dépôt Legacy2UEFI ne publie pas encore d'image `.img` dans ses releases : le bouton
  « Vérifier le dépôt » signale la dernière version mais l'image doit être choisie
  localement.
- Interface en français uniquement pour l'instant.

## Licence

GPL-3.0-or-later, comme le projet Legacy2UEFI.
