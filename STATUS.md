# État du projet

_Mis à jour par Claude à chaque PR._

## Ce qui marche
- Squelette Bevy 0.20 : fenêtre, sol, kart provisoire qui tourne sur lui-même, test headless.
- CI : fmt, clippy, tests, check wasm, puis build trunk et déploiement GitHub Pages à chaque merge sur `main`.

## Décisions
- 2026-10-12 : Bevy 0.20, physique bevy_rapier3d 0.37. Avian, lightyear et bevy_ggrs ne supportent pas encore 0.20.
- 2026-10-12 : réseau P2P en maillage complet via matchbox_socket. Chaque joueur fait autorité sur son kart, l'hôte arbitre la course. Pas de serveur de jeu (voir `docs/ARCHITECTURE.md`).
- 2026-10-12 : WebGL2 par défaut pour la compatibilité, WebGPU à évaluer plus tard.
- 2026-10-12 : formes primitives d'abord, packs d'assets CC0 ensuite (issue dédiée).

## Prochaines étapes
- Milestone 1 « Ça roule » : issues #1 à #6.
- Milestone 2 « À plusieurs » : #7 à #10 (#10 attend un compte Cloudflare).
- Milestone 3 « Fun et habillage » : #11 (attend les assets), #12.

Les textes des issues de départ sont dans `.github/backlog/` (créées par `scripts/create-backlog-issues.sh`).

## Questions pour Max
_(aucune)_

## Métriques
- Taille wasm : _à mesurer au premier déploiement_
