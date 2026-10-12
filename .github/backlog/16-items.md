Système d'objets 1/2, en solo : les règles de Mario Kart 8 Deluxe avec nos objets originaux. Voir `docs/GAME_DESIGN.md` § 8.

## À faire
- **Boîtes à objets** placées dans les données du circuit ; elles réapparaissent après environ 2 s. **Doubles boîtes** qui donnent 2 objets.
- **Roulette** d'environ 1,5 s, avec un tirage selon la **distance au leader** (table RON du game design ; les objets pas encore implémentés sont retirés et leur poids redistribué).
- **Deux emplacements** (main + réserve), affichés dans le HUD.
- **Traîner derrière soi** en maintenant le bouton Objet (pelure, pelote), comme bouclier contre ce qui arrive par l'arrière.
- **Réactions aux coups** : tête-à-queue (≈ 1 s), culbute (≈ 1,6 s), puis invincibilité de 1 à 2 s selon la stat.
- Objets de phase 1 :
  - Pelure (x1, x3) ;
  - Pelote de laine (x1, x3), qui rebondit ; le x3 tourne autour du kart ;
  - Abeille (x1, x3), à tête chercheuse vers le kart juste devant ;
  - Piment (x1, x3) ;
  - Croquette dorée.
- Lancer devant (par défaut) ou derrière (direction vers l'arrière + Objet).
- Les bots ramassent et utilisent les objets, de façon simple mais crédible.
- Visuels provisoires lisibles (formes et couleurs distinctes).

## Critères d'acceptation
- Tests : tirage statistique conforme à la table (sur 10 000 tirages), gestion des deux emplacements, une pelure traînée bloque une pelote venant de l'arrière, et une abeille touche bien le kart devant.
