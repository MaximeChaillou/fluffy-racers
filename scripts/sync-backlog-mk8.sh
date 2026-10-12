#!/usr/bin/env bash
# One-shot: aligns the backlog with docs/GAME_DESIGN.md (Mario Kart 8 Deluxe reference).
# Updates issues #1, #3, #4, #5, #6, #9 and creates the new ones.
# Usage (repo root, gh logged in): ./scripts/sync-backlog-mk8.sh
set -euo pipefail
cd "$(dirname "$0")/.."
B=.github/backlog

gh issue edit 1 --title "Kart arcade jouable (physique, états, boosts, hors-piste)" --body-file $B/01-kart.md
gh issue edit 3 --body-file $B/03-track.md
gh issue edit 4 --title "Drift et mini-turbos à 3 paliers (bleu, orange, violet)" --body-file $B/04-drift.md
gh issue edit 5 --title "Course : 12 coureurs, 3 tours, classement, dépanneur, HUD" --body-file $B/05-race.md
gh issue edit 6 --body-file $B/06-bots.md
gh issue edit 9 --title "Réseau 3/3 : course synchronisée (objets et croquettes compris)" --body-file $B/09-net-race.md

create() { gh issue create --title "$1" --label "$2" --body-file "$B/$3"; }
create "Départ fusée, figures, aspiration, panneaux de boost" milestone-1 13-techniques.md
create "Croquettes (équivalent des pièces)"                   milestone-1 14-croquettes.md
create "Statistiques personnages et karts, 3 gabarits"        milestone-1 15-stats.md
create "Objets 1/2 : boîtes, roulette, objets de base"        milestone-1 16-items.md
create "Aides de conduite et regarder derrière"               milestone-1 17-assists.md
create "Objets 2/2 : objets avancés"                          milestone-2 18-items-2.md
create "Grand Prix, classes de vitesse, contre-la-montre"     milestone-3 19-grand-prix.md
create "Anti-gravité, planeur, sous l'eau"                    milestone-3 20-environments.md
create "Objets de phase 3 et mode Bataille"                   milestone-3 21-battle.md

echo "Backlog aligné sur docs/GAME_DESIGN.md."
