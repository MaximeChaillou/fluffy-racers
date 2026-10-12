Les règles d'une course : tours, classement et chrono.

## À faire
- Module `race/`, avec une **logique pure** testable (sans Bevy) :
  - progression d'un kart = (tour, dernier checkpoint, distance au suivant) ;
  - classement en temps réel ;
  - les checkpoints doivent être passés dans l'ordre (anti-raccourci).
- États de la course : grille → compte à rebours 3-2-1 → course (3 tours) → arrivée → écran de résultats → rejouer.
- Respawn automatique au dernier checkpoint en cas de chute ou de kart bloqué, plus une touche R.
- HUD `bevy_ui` : position (1er/12), tour (2/3), chrono, meilleur tour.

## Critères d'acceptation
- Tests unitaires du classement : dépassements, kart qui coupe, kart qui recule.
- Une course complète est jouable en solo.
