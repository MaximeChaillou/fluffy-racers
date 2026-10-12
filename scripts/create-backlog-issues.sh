#!/usr/bin/env bash
# One-shot: creates labels and the initial backlog issues from .github/backlog/.
# Usage (repo root, with gh logged in): ./scripts/create-backlog-issues.sh
set -euo pipefail
cd "$(dirname "$0")/.."

gh label create milestone-1 --color 0E8A16 --description "Ça roule (solo)" --force
gh label create milestone-2 --color 1D76DB --description "À plusieurs (réseau)" --force
gh label create milestone-3 --color 5319E7 --description "Fun et habillage" --force
gh label create needs-max  --color D93F0B --description "Max doit faire une action avant" --force

create() { gh issue create --title "$1" --label "$2" --body-file ".github/backlog/$3"; }

create "Kart arcade jouable (physique + entrées)"            milestone-1           01-kart.md
create "Caméra de poursuite"                                 milestone-1           02-camera.md
create "Circuit de test en primitives (généré par spline)"   milestone-1           03-track.md
create "Drift et mini-turbo"                                 milestone-1           04-drift.md
create "Course : tours, checkpoints, classement, HUD"        milestone-1           05-race.md
create "Bots et test de charge 12 karts"                     milestone-1           06-bots.md
create "Réseau 1/3 : rooms et connexion P2P"                 milestone-2           07-net-rooms.md
create "Réseau 2/3 : synchronisation des karts"              milestone-2           08-net-sync.md
create "Réseau 3/3 : course synchronisée"                    milestone-2           09-net-race.md
create "Signalisation de prod sur Cloudflare (gratuit)"      milestone-2,needs-max 10-signaling.md
create "Vrais modèles 3D (packs CC0)"                        milestone-3,needs-max 11-assets.md
create "Personnages mous : wobble procédural"                milestone-3           12-wobble.md

echo "Backlog créé."
