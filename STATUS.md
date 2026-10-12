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
- 2026-10-12 : référence de gameplay = Mario Kart 8 Deluxe (mécaniques et valeurs dans `docs/GAME_DESIGN.md`), avec un contenu 100 % original.

## Prochaines étapes
- Milestone 1 « Ça roule » (solo) :
  - kart, caméra, circuit, drift à 3 paliers, course, bots ;
  - puis départ fusée, figures et aspiration ; croquettes ; stats et gabarits ; objets 1/2 ; aides de conduite.
- Milestone 2 « À plusieurs » : réseau 1/3 à 3/3, signalisation (attend un compte Cloudflare), objets 2/2.
- Milestone 3 « Contenu » : vrais modèles (attend les assets), personnages mous, Grand Prix et classes, anti-gravité / planeur / eau, objets de phase 3 et Bataille.

Le gameplay de référence est décrit dans `docs/GAME_DESIGN.md` (inspiré de Mario Kart 8 Deluxe). Les textes des issues sont dans `.github/backlog/`.

## Questions pour Max
_(aucune)_

## Métriques
- Taille wasm : _à mesurer au premier déploiement_
