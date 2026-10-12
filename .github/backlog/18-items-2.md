Système d'objets 2/2 : les objets de phase 2. Voir `docs/GAME_DESIGN.md` § 8.

## À faire
- Piment doré, Bombe à confettis, Chantilly, Super câlin, Fusée à confettis, Dégonflage, Oreiller géant, Klaxon-canard.
- Règles de l'**oreiller géant** : un seul à la fois sur la piste, recharge de 30 s, aucun avant 20 s de course ; le klaxon-canard le détruit.
- Synchronisation réseau de chaque objet selon `docs/ARCHITECTURE.md` (le lanceur est autoritaire, la victime décide).
- Mise à jour de la table de distribution.

## Critères d'acceptation
- Tests unitaires par objet : effet, durée et interaction avec l'invincibilité.
- La PR décrit comment tester chaque objet (touche de debug pour forcer un objet).
