Un premier circuit fermé, en formes primitives, généré depuis des données.

## À faire
- Module `track/` : un circuit est défini par des points de contrôle (spline Catmull-Rom fermée) avec, par point, la largeur et l'inclinaison (banking).
- Génération du mesh de la route, des bordures et des murs bas, plus un collider trimesh Rapier.
- Format de données en RON (`assets/tracks/*.ron`) : on doit pouvoir créer un nouveau circuit sans toucher au code.
- Circuit de test : une longue ligne droite, une épingle, un virage relevé, un saut, une chicane.
- Placement automatique des checkpoints le long de la spline, plus la ligne de départ et 12 places sur la grille.
- La spline sert aussi de « ligne de course » aux bots (#6).
- **Éléments de gameplay dans les données du circuit**, même s'ils ne sont pas encore utilisés : zones hors-piste, tremplins à figure, panneaux d'accélération, emplacements des boîtes à objets (simples et doubles) et des croquettes, points de réapparition du dépanneur.
- Profil inspiré des circuits de jeux de kart (sans en copier un) : largeur généreuse (≈ 14 m), virages lisibles, une ligne idéale qui récompense le drift.
- Décor minimal : herbe, quelques blocs ou arbres en primitives.

## Critères d'acceptation
- Test : le mesh généré est fermé et les checkpoints sont dans l'ordre le long du circuit.
- Le kart de #1 peut faire un tour complet.
