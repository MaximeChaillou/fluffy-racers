Les règles d'une course : 12 coureurs, 3 tours, classement et chrono. Voir `docs/GAME_DESIGN.md` § 1 et 2.

## À faire
- Module `race/`, avec une **logique pure** testable :
  - progression d'un kart = (tour, dernier checkpoint, distance au suivant) ;
  - classement en temps réel ;
  - les checkpoints doivent être passés dans l'ordre (anti-raccourci) ;
  - **distance au leader** le long de la ligne de course (servira à la distribution des objets).
- États de la course : grille de 12 → compte à rebours 3-2-1-GO (1 s par étape) → course de 3 tours → arrivée → résultats → rejouer.
- **Pélican dépanneur** : en cas de chute ou de kart bloqué, il ramène le kart au dernier point sûr, avec environ 2,5 s de pénalité. Animation provisoire.
- HUD `bevy_ui` : position (1er/12), tour (2/3), chrono, meilleur tour, et un emplacement prévu pour les objets et les croquettes.

## Critères d'acceptation
- Tests unitaires : classement (dépassements, kart qui coupe, kart qui recule) et distance au leader.
- Une course complète est jouable en solo.
