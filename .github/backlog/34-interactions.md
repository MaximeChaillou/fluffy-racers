Implémentation des interactions entre karts demandées par Max : attraper un autre kart ou personnage, et donner un coup pour le faire dévier de sa trajectoire.

La mécanique est décrite dans `docs/GAME_DESIGN.md`, section « Interactions entre karts » (ajoutée par #33), et son fonctionnement réseau dans `docs/ARCHITECTURE.md`. Pas urgent : à faire après les autres issues du milestone 1, mais **avant le réseau** (#7 à #9), pour que la synchronisation l'intègre dès le départ.

## Dépend de
- #33 (game design et architecture) ;
- #14 (croquettes : un coup fait perdre une croquette) ;
- #15 (le Poids module la force).

## À faire
- **Entrée** :
  - champ `interact` dans `KartInput` ;
  - touche clavier et bouton manette, comme indiqué au § 13 du game design ;
  - le kart ne lit jamais le clavier.
- **Coup** :
  - choisir la cible à portée sur le côté ;
  - appliquer à la cible une impulsion latérale et une perte de vitesse, et lui faire perdre une croquette ;
  - recharge ;
  - effet du Poids.
- **Attraper** :
  - lien élastique entre les deux karts, durée maximale ;
  - remorquage et frein de la cible ;
  - la cible se dégage par un saut ou un drift ;
  - en lâchant, on peut jeter la cible sur le côté.
- **Immunités** : impossible pendant le dépanneur, l'état Touché ou avant le GO ; une cible invincible est immunisée.
- **Bots** : ils utilisent les interactions selon leur niveau, avec un réglage `BotSkill`.
- **Ressenti (provisoire)** :
  - une animation simple de bras ou de patte en primitives ;
  - un tremblement d'écran ;
  - le mini-ragdoll arrivera avec #12.
- **Réglages** : toutes les valeurs dans une Resource de réglage.

## Critères d'acceptation
Tests de scénario :
- coup sur un kart à portée → il dévie latéralement, ralentit et perd 1 croquette ;
- coup hors de portée → aucun effet sur les autres karts ;
- attrape maintenue → les deux karts restent liés, au plus pendant la durée maximale ;
- la cible saute → le lien se rompt ;
- coup ou attrape pendant la recharge → rien ne se passe ;
- les courses de bots existantes tiennent toujours leurs seuils.
