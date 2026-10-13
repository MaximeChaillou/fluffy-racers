# Game design : Fluffy Racers

**Référence de gameplay : Mario Kart 8 Deluxe (MK8DX).** L'objectif est de retrouver ses sensations : drift et mini-turbos, départ fusée, figures, aspiration, pièces, objets dosés selon la distance au leader, 12 coureurs.

En plus de ces mécaniques, Fluffy Racers a les siennes : les **interactions entre karts** (coup de patte, attrape, § 15).

Ce document est **la source de vérité** pour le gameplay. En cas de doute, il l'emporte sur le texte d'une issue.

> **Propriété intellectuelle.** Les *mécaniques* de jeu ne sont pas protégées : on peut les reprendre. Tout le reste doit être **original** : noms des objets et des personnages, visuels, circuits, musiques, sons, interface. N'utilise jamais un nom, un personnage, un objet, un logo ou un tracé de circuit de Nintendo. Le tableau des objets plus bas donne nos équivalents.

Les valeurs marquées **(MK8DX)** viennent de sources publiques sur le jeu. Celles marquées **(à régler)** sont nos propres points de départ : elles vont dans les Resources de réglage et seront ajustées en jouant.

---

## 1. Format de course

| Élément | Valeur |
|---|---|
| Coureurs | **12** : joueurs + bots (MK8DX) |
| Tours | **3** (MK8DX, la plupart des circuits) |
| Grand Prix | 4 courses ; points du 1er au 12e : **15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1** (MK8) |
| Classes de vitesse | 50cc, 100cc, **150cc (par défaut)**, 200cc, plus un mode miroir. Multiplicateurs de vitesse max : 0,75 / 0,87 / 1,0 / 1,25 (à régler) |
| Départ | Grille en quinconce, compte à rebours « 3, 2, 1, GO » d'une seconde par étape |
| Modes | Course libre, Grand Prix, Contre-la-montre (3 piments, fantôme), En ligne (jusqu'à 12), Bataille (plus tard) |

## 2. Conduite de base

C'est de l'arcade : le kart est piloté par une **machine à états** (au sol, saut, drift, boost, en l'air, figure, touché, dépannage) et des **courbes de réglage**. La physique Rapier gère les collisions et le contact avec le sol, mais **ne décide pas du ressenti**.

- **Vitesse max 150cc** : 26 m/s, soit environ 94 km/h, pour un kart de 2 m (à régler). Accélération de 0 à la vitesse max en environ 2,5 s pour un kart moyen (à régler, dépend de la stat Accélération).
- **Frein / marche arrière** : un seul bouton. Il freine ; à l'arrêt, il recule.
- **Hors-piste** (herbe, sable) : la vitesse max est très réduite, environ 55 % (à régler). Un boost l'ignore pendant sa durée (MK8DX : traverser un raccourci avec un champignon).
- **Murs** : on rebondit et on perd de la vitesse, mais on ne s'arrête jamais net. On évite tout blocage.
- **Panneaux d'accélération** au sol : boost court, environ 1 s (à régler).
- **Chute hors du circuit** : un **pélican dépanneur** ramasse le kart et le repose au dernier point sûr. Perte d'environ 2,5 s (à régler). MK8DX dit seulement qu'on « perd beaucoup de temps ».
- **Regarder derrière** : un bouton dédié (MK8DX), pour voir arriver les objets.

### Boosts : un seul système

Mini-turbos, piments, départ fusée, figures, aspiration, panneaux : tous passent par un composant `Boost { strength, remaining }`. Pendant un boost, la vitesse max est augmentée (par exemple +30 % pour un piment, à régler) et l'accélération est forte. Quand deux boosts se cumulent, on garde la plus forte intensité et la plus longue durée restante.

## 3. Drift et mini-turbos (le cœur du jeu)

D'après MK8DX :

1. **Bouton Saut/Drift** (le même bouton sert aussi aux figures) : petit **saut**. Si on braque pendant le saut et qu'on garde le bouton enfoncé à l'atterrissage, on entre en **drift** du côté du braquage. Il faut une vitesse minimale.
2. **En drift**, le kart garde un angle de glisse. La direction **resserre ou élargit** la courbe, mais on ne peut pas changer de côté.
3. **Charge du mini-turbo**, comptée à chaque tick (60 Hz) :
   - en braquant **vers l'intérieur** (≥ 0,5) : **+5 unités/tick** ; sinon **+2 unités/tick** (MK8DX) ;
   - en direction assistée (« auto-drift ») : 2 et 0,8 unités/tick (MK8DX).
4. **Paliers et étincelles** (MK8DX) :

| Palier | Seuil (à régler, modulé par la stat MT) | Durée au plus vite | Étincelles | Durée du boost (MK8DX) |
|---|---|---|---|---|
| Mini-turbo | 270 | ≈ 0,9 s | **bleues** | **0,62 s** |
| Super mini-turbo | 570 | ≈ 1,9 s | **orange** | **1,67 s** |
| Ultra mini-turbo | 870 | ≈ 2,9 s | **violettes** | **2,63 s** |

   Les seuils 270 et 570 sont ceux de MK8 sur Wii U. Dans MK8DX, ils dépendent de la stat MT ; 870 est notre extrapolation pour le 3e palier.
5. **Relâcher le bouton** déclenche le boost du palier atteint. Relâcher avant le premier palier ne donne rien.
6. L'ultra mini-turbo est **désactivé quand la direction assistée est active** (MK8DX).
7. **En 200cc**, freiner pendant un drift resserre la courbe sans casser le drift.

## 4. Départ fusée

- MK8DX : il faut accélérer **juste après l'apparition du « 2 »**. Plus on est précis, plus le boost dure. Trop tôt, on **cale**.
- Notre implémentation (à régler) :
  - on mesure `t` = temps entre l'apparition du « 2 » et le premier appui sur l'accélérateur ;
  - appui avant le « 2 » : **calage** de 1 s (moteur qui crache, kart immobile) ;
  - `t ∈ [0 ; 0,25 s]` : boost **maximal de 2,5 s** ;
  - ensuite, la durée baisse linéairement jusqu'à 0,5 s à `t = 1 s` ;
  - appui après le « 1 » : pas de boost.
- Retour visuel : des flammes d'échappement dont la taille dépend de la qualité du départ.

## 5. Figures sur tremplin (jump boost)

- Appuyer sur Saut/Drift au moment où le kart quitte un tremplin ou une arête marquée (fenêtre d'environ 0,2 s, à régler) déclenche une **figure** (animation). On obtient un **boost court à l'atterrissage**, d'environ 0,5 à 0,8 s selon la stat MT (MK8DX : « short boost on landing »).
- Les tremplins sont marqués dans les données du circuit.

## 6. Aspiration (slipstream)

- MK8DX : l'effet démarre **à environ deux longueurs** derrière un kart. Rester dans son sillage donne un boost.
- Notre implémentation (à régler) : un kart à moins de 5 m derrière un autre, dans un cône de ±12°, au-dessus de 60 % de sa vitesse max, charge une jauge. Après **1,2 s** de charge, il reçoit un boost de **1 s**.
- Retour visuel : des lignes de vent autour du kart pendant la charge.

## 7. Croquettes (l'équivalent des pièces)

- MK8DX : chaque pièce augmente la **vitesse max**, **jusqu'à 10 pièces**.
- Chez nous : 10 croquettes maximum, chacune donne **+1 % de vitesse max** (à régler). Ramasser une croquette donne un mini-boost de 0,3 s.
- On **perd 3 croquettes** quand on est touché par un objet, et 1 quand on se fait percuter ou qu'on reçoit un coup de patte (§ 15) (à régler). Les croquettes perdues s'éparpillent au sol.
- Les croquettes du circuit réapparaissent après environ 10 s.

## 8. Objets

### Règles (MK8DX)
- **Boîtes à objets** sur la piste. On roule dessus → roulette d'environ 1,5 s (à régler) → objet. La boîte réapparaît après environ 2 s.
- **Doubles boîtes** : elles donnent deux objets. On les place à des endroits plus risqués.
- **Deux emplacements** : un objet en main et un en réserve.
- **Traîner un objet derrière soi** : maintenir le bouton Objet garde une pelure ou une pelote derrière le kart, comme bouclier.
- **La distribution dépend de la distance au leader, pas de la position** (MK8DX). Plus on est loin du 1er, plus les objets sont puissants. MK8 utilise 10 tranches de distance.
- **Touché** : **tête-à-queue** (≈ 1 s, objets légers) ou **culbute** (≈ 1,6 s, explosions et gros objets). Ensuite, une **invincibilité** de 1 à 2 s, qui dépend de la stat Invincibilité (MK8DX : 1,5 s environ en Bataille).

### Nos objets (originaux)

Chaque objet reprend la *fonction* d'un objet classique de jeu de kart, avec un nom et un visuel à nous.

| Notre objet | Fonction | Phase |
|---|---|---|
| **Pelure** (x1, x3) | Piège au sol : tête-à-queue. Peut se traîner derrière soi. | 1 |
| **Pelote de laine** (x1, x3) | Projectile en ligne droite qui rebondit sur les murs : culbute. Peut se traîner ; le x3 tourne autour du kart. | 1 |
| **Abeille** (x1, x3) | Projectile à tête chercheuse vers le kart juste devant. | 1 |
| **Piment** (x1, x3) | Boost d'environ 1,2 s. | 1 |
| **Croquette dorée** | Ajoute 2 croquettes. | 1 |
| **Piment doré** | Boosts illimités pendant environ 7 s. | 2 |
| **Bombe à confettis** | Lancée devant ou derrière, explose en zone : culbute. | 2 |
| **Chantilly** | Projette de la crème sur l'écran des coureurs devant soi. Pour les bots, la trajectoire devient moins précise. | 2 |
| **Super câlin** | Environ 7 s d'invincibilité, avec une vitesse augmentée et les karts percutés renversés. | 2 |
| **Fusée à confettis** | Pilote automatique très rapide qui suit la ligne de course pendant environ 6 s et renverse ceux qu'il touche. | 2 |
| **Dégonflage** | Tous les adversaires rapetissent et ralentissent ; leurs objets tombent. Les premiers restent petits plus longtemps. | 2 |
| **Oreiller géant** | Vole vers le **leader** et l'écrase (culbute, petite zone). **Un seul à la fois** sur la piste, temps de recharge de 30 s, aucun avant 20 s de course. | 2 |
| **Klaxon-canard** | Onde de choc autour du kart qui détruit les objets et projectiles proches, **oreiller géant compris**. | 2 |
| **Pie voleuse** | Rend invisible environ 3 s et vole l'objet d'un adversaire. | 3 |
| **Piñata** | Fait apparaître 8 objets autour du kart, à utiliser un par un. | 3 |
| **Boomerang-os** | Boomerang qui revient, utilisable 3 fois. | 3 |

### Distribution (première proposition, à régler)

La distance est mesurée le long de la ligne de course, en mètres, jusqu'au leader. Le leader lui-même utilise la première ligne.

| Distance au leader | Objets probables |
|---|---|
| leader / < 15 m | Pelure 30 %, Pelote 25 %, Croquette dorée 20 %, Pelure x3 10 %, Klaxon-canard 10 %, Bombe 5 % |
| 15 – 50 m | Pelote 20 %, Abeille 25 %, Pelure 15 %, Piment 15 %, Bombe 10 %, Pelote x3 10 %, Chantilly 5 % |
| 50 – 120 m | Abeille 25 %, Abeille x3 15 %, Piment 20 %, Piment x3 15 %, Chantilly 10 %, Oreiller géant 5 %, Pelote x3 10 % |
| 120 – 250 m | Piment x3 25 %, Abeille x3 20 %, Super câlin 15 %, Piment doré 15 %, Oreiller géant 10 %, Fusée 10 %, Dégonflage 5 % |
| > 250 m | Super câlin 25 %, Fusée 25 %, Piment doré 20 %, Piment x3 15 %, Dégonflage 10 %, Oreiller géant 5 % |

La table est une donnée de configuration (RON), pas du code. Tant qu'un objet n'est pas implémenté, on le retire de la table et on redistribue son poids proportionnellement sur les autres lignes.

## 9. Statistiques des personnages et karts

Modèle MK8DX :

- **Stats** : Vitesse, Accélération, Poids, Maniabilité, Adhérence (hors-piste et surfaces glissantes), **Mini-turbo** et **Invincibilité** (stats cachées). La vitesse et la maniabilité existent en 4 variantes de terrain (sol, eau, air, anti-gravité). On commence avec le sol uniquement.
- **Somme de 4 éléments** : personnage + kart + roues + aile. Chaque élément vaut de 0 à 10 points par stat. Le total est borné à un **niveau de 0 à 20**. Le menu affiche `(niveau + 3) / 4`, soit de 0,75 à 5,75.
- **Effets** : chaque niveau est converti en paramètres par interpolation linéaire. Exemple : vitesse max = base × lerp(0,92 ; 1,08 ; niveau / 20) (à régler).
  - **Poids** : qui pousse qui dans les collisions, et la force des interactions entre karts (§ 15) : un lourd frappe et jette plus fort, et freine davantage le kart qu'il attrape ; un léger se fait remorquer sans presque freiner sa cible.
  - **Maniabilité** : vivacité de la direction et rayon de drift.
  - **Mini-turbo** : seuils de charge, puis durée et force des boosts de drift et de figure.
  - **Invincibilité** : durée de la protection après avoir été touché par un objet (§ 8).
- **Trois gabarits**, avec le compromis classique (MK8DX) : les légers ont plus d'accélération et de maniabilité, les lourds plus de vitesse et de poids.

Roster de départ (original, 3 par gabarit) :

| Gabarit | Animaux |
|---|---|
| Léger | souris, poussin, hamster |
| Moyen | chat, lapin, renard |
| Lourd | ours, morse, vache |

## 10. Environnements spéciaux (phase 3)

- **Anti-gravité** : zones marquées où les roues pivotent et où l'on roule sur les murs et les plafonds. **Se percuter donne un boost aux deux karts** (spin boost, MK8DX). Des plots dédiés donnent aussi un boost.
- **Planeur** : un tremplin spécial ouvre une aile ; on dirige alors haut, bas, gauche et droite avec les stats « air ».
- **Sous l'eau** : on passe aux stats « eau », plus lentes et plus flottantes.

## 11. Aides et accessibilité (MK8DX)

- **Direction assistée** : garde le kart sur la piste dans les virages difficiles, avec un témoin visible sur le kart. Elle désactive l'ultra mini-turbo.
- **Accélération automatique**.
- Les deux se règlent par joueur. Elles sont désactivées par défaut en ligne, mais restent autorisées.

## 12. Bots

- Ils produisent un `KartInput` comme un joueur et suivent la ligne de course avec des variations.
- **Trois niveaux de difficulté** : précision, usage du drift (palier visé), usage des objets et des interactions entre karts (coup de patte, attrape : voir § 15).
- **Léger rattrapage** : les bots très en retard gagnent un peu de vitesse max, et ceux très en tête en perdent un peu (≤ 3 %, à régler). L'essentiel du rattrapage vient de la distribution des objets.

## 13. Commandes (par défaut)

| Action | Clavier | Manette |
|---|---|---|
| Accélérer | Haut / W / Z | A |
| Freiner / reculer | Bas / S | B |
| Diriger | Gauche/Droite, A/D, Q/D | Stick gauche |
| Saut / drift / figure | Espace | RT |
| Objet (maintenir = traîner) | Shift ou E | LB ou LT |
| Interaction (appui court = coup de patte, maintien = attraper, § 15) | X ou F | RB |
| Regarder derrière | C | X / Y |
| Pause | Échap | Start |

- Les touches sont des **positions physiques** : W/A/S/D se lisent Z/Q/S/D sur un clavier AZERTY. X, F, E et C portent la même lettre en AZERTY et en QWERTY.
- **Interaction** : X se tient de la main gauche à côté de Shift et d'Espace quand on conduit aux flèches ; F est l'équivalent de E pour qui conduit avec W/A/S/D (Z/Q/S/D). À la manette, RB se tient à l'index pendant que le pouce garde A (accélérer) : RB quitte donc le saut/drift, qui reste sur RT.
- **Ni Ctrl ni Alt** : Ctrl + W (Cmd + W sur Mac) ferme l'onglet et le navigateur ne laisse pas la page l'empêcher ; Alt ouvre les menus de certains navigateurs.
- Touches de debug déjà prises : B (boost), R (pélican), F3 (overlay) ; Entrée et Espace relancent la course depuis le classement.

## 14. Ressenti (« juice »)

C'est une grande partie de ce qui fait le plaisir de MK8DX. Chaque mécanique a un **retour clair** :

- des étincelles par palier de drift ;
- des flammes de boost dont la taille suit l'intensité ;
- un FOV qui s'élargit avec la vitesse ;
- des lignes de vent pour l'aspiration ;
- un « pop » et un son à chaque croquette ;
- une vibration de la manette ;
- un léger tremblement d'écran sur les chocs ;
- un cri d'animal quand le personnage est touché ou fait une figure ;
- **coup de patte** (§ 15) : la patte se lève dès l'appui ; à l'impact, un « pof » cartoon (petite étoile), le mini-ragdoll et le cri de la cible, un léger tremblement d'écran et une vibration chez la victime ;
- **attrape** : un bras élastique tendu entre les deux karts, qui s'étire avec la distance et revient en « boing » au lâcher ; le personnage attrapé se retourne, outré ;
- **coup ou attrape dans le vide** : le personnage se penche trop hors du kart et mouline des bras ;
- un témoin de recharge du bouton Interaction dans le HUD (une patte qui se remplit).

**Lisibilité** : couleurs vives, silhouettes claires. Les objets dangereux se reconnaissent instantanément.

## 15. Interactions entre karts

Une mécanique propre à Fluffy Racers : depuis son kart, le personnage peut **donner un coup de patte** à un voisin pour le faire dévier, ou **l'attraper** pour se faire remorquer. C'est de l'humour physique (bras élastiques, déséquilibres, ragdoll) autant qu'un outil de course.

Toutes les valeurs vont dans une Resource de réglage. Le fonctionnement en ligne est décrit dans `docs/ARCHITECTURE.md` § « Interactions entre karts » : la victime applique le coup à son propre kart, comme pour les objets.

### Bouton Interaction

- **Appui court**, relâché avant 0,2 s (à régler) : **coup de patte**, donné au relâchement.
- **Maintien** au-delà de 0,2 s : **attraper**. La prise dure tant qu'on tient le bouton, 2 s au plus.
- La patte se lève **dès l'appui** : on voit tout de suite que l'action part, même si le coup n'est donné qu'au relâchement.
- **Une seule recharge** pour le bouton (à régler) : 1,5 s après un coup de patte (réussi ou dans le vide) ou une attrape ratée, 3 s après la fin d'une attrape. Pendant la recharge, le bouton ne fait rien.

### Portée et choix de la cible

Les distances sont mesurées dans le repère du kart, jusqu'au centre de la cible (à régler) :

- **Sur le côté** (coup et attrape) : au plus **2,5 m entre les flancs** des deux karts. Le centre de la cible est entre 0,7 m et 3,9 m de l'axe du kart, entre 2,5 m derrière et 2,5 m devant son centre, et à moins de 1,5 m de différence de hauteur. La patte est élastique : la portée est généreuse, ce qui absorbe aussi la latence en ligne.
- **Juste devant** (attrape seulement) : au plus 2,5 m entre l'avant du kart et l'arrière de la cible, centre à moins de 1,4 m de côté. On s'accroche au kart de devant pour se faire remorquer.
- **Cible** : le kart à portée le plus proche, centre à centre. S'il y en a des deux côtés, le côté vers lequel on braque (au moins 0,3) l'emporte.
- Un kart immunisé (voir plus bas) n'est jamais choisi : la patte passe au suivant.

### Coup de patte

- **Effet sur la cible** (à régler) :
  - une **poussée latérale** dans la direction du coup : 7 m/s qui s'éteignent en 0,4 s, soit un écart d'environ 1,5 m. Le grip et le drift ne l'effacent pas ;
  - une **perte de 10 % de sa vitesse** ;
  - **1 croquette perdue**, comme un choc entre karts (§ 7), avec la même protection d'1 s : un coup suivi du choc qu'il provoque ne coûte qu'une croquette ;
  - le **mini-ragdoll** visuel du personnage, son cri et un « pof » (§ 14).
- **La cible garde le contrôle** : ni tête-à-queue ni état Touché. Son drift, sa charge et son boost continuent.
- Ensuite, la cible est **protégée des coups de patte pendant 1 s** (à régler), mais pas des objets : on ne peut pas s'acharner à plusieurs sur le même kart.
- **Coup dans le vide** (personne à portée) : la patte frappe l'air du côté vers lequel on braque (au neutre : à droite). Le personnage part en déséquilibre : il se penche hors du kart en moulinant des bras, et le kart glisse d'environ 0,3 m de ce côté (à régler). Pas de perte de vitesse : la punition, c'est la recharge.

### Attraper

- **Prise** : au maintien, la patte s'allonge et attrape le premier kart qui passe à portée pendant 0,5 s (à régler). Si personne ne passe, c'est une **griffe dans le vide**, avec le même déséquilibre que le coup dans le vide.
- **Lien élastique** entre les deux karts, **2 s au plus**. Sa longueur de repos est la distance au moment de la prise. Il casse s'il s'étire de plus de 2,5 m (à régler).
- **L'attrapeur est remorqué** : sa vitesse rejoint celle de la cible en 0,5 s environ (à régler), sans jamais la dépasser. Il garde sa direction. On s'accroche pour rattraper, pas pour doubler : pour doubler, il faut lâcher ou jeter.
- **La cible est un peu freinée** : elle reçoit 25 % de la force du lien (à régler), seulement ce qui la retient ou la tire sur le côté, jamais vers l'avant. Objectif de réglage : à poids égal, une prise complète de 2 s coûte environ 10 % de sa vitesse à une cible lancée.
- **Se dégager** : la cible saute (bouton Saut/Drift, donc aussi en partant en drift). Le lien casse à l'atterrissage, environ 0,35 s après l'appui, et l'attrapeur part en déséquilibre.
- **Lâcher** : on relâche le bouton, ou les 2 s sont écoulées. Si l'on braque franchement (au moins 0,5) au moment de lâcher, après au moins 0,5 s de prise (à régler), on **jette** la cible de ce côté : c'est un coup de patte dont la poussée est 1,5 fois plus forte, avec la même perte de vitesse, la croquette et la protection d'1 s. Sinon, la cible est simplement relâchée.
- **Le lien casse aussi** si l'un des deux karts est touché par un objet, part avec le pélican ou franchit l'arrivée, ou si l'attrapeur saute (c'est un lâcher, sans jet).
- **Un lien à la fois** : un kart qui attrape ou qui est attrapé ne peut ni attraper ni être attrapé. Il peut toujours recevoir le coup de patte d'un troisième kart (le lien tient) et utiliser ses objets.

### Effet du Poids

Le niveau de Poids (0 à 20, § 9) module la **force**, jamais la portée ni les croquettes (à régler) :

- **Coup de patte et jet** : poussée × lerp(0,7 ; 1,3 ; (Poids du frappeur − Poids de la cible + 20) / 40). À poids égal, ×1 ; le plus lourd sur le plus léger, ×1,3 ; l'inverse, ×0,7.
- **Lien** : chaque kart reçoit la force du lien divisée par lerp(0,8 ; 1,2 ; son Poids / 20). Une souris accrochée à un ours se fait remorquer sans presque le ralentir ; un ours accroché à une souris la freine nettement.

### Immunités

| État du kart | Peut frapper | Peut attraper | Peut recevoir un coup | Peut être attrapé |
|---|---|---|---|---|
| Au sol | oui | oui | oui | oui |
| Petit saut (Saut/Drift) | oui | non | oui | non |
| Drift | oui | non | oui | non |
| En l'air (tremplin), figure | non | non | non | non |
| Touché (§ 8) | non | non | non | non |
| Invincible (après un objet, super câlin…) | oui | oui | non | non |
| Pélican dépanneur | non | non | non | non |
| Calage du départ (§ 4) | non | non | oui | oui |
| Avant le GO, après l'arrivée | non | non | non | non |

Avant le GO, le verrou de la course coupe déjà toutes les entrées. Le fantôme du contre-la-montre n'est pas un kart : il ne compte pas.

### Drift, saut et boosts

- **On peut frapper en drift**, sans casser le drift ni sa charge. Les karts se regroupent dans les virages, où le drift est permanent : l'interdire rendrait le coup inutilisable là où il est le plus drôle, et casser le drift en ferait un piège. Le prix est la recharge. Le braquage du drift désigne le côté intérieur quand il y a des karts des deux côtés.
- **Recevoir un coup en drift** : la poussée s'ajoute à la glisse, le drift et sa charge continuent. Comme après un choc, le drift ne casse que si la vitesse tombe trop bas (sous 7 m/s) ou après 0,5 s en l'air.
- **Pas d'attrape en drift ni pendant un saut**, ni sur un kart qui drifte ou saute : le saut et le drift sont justement la façon de se dégager.
- **Boosts** : un boost ne protège de rien et n'empêche rien. Le coup ne coupe pas le boost de la cible. Accroché à un kart en boost, on file à sa vitesse : c'est le meilleur moment pour attraper. Le lien, lui, ne donne jamais de boost.

### Bots

Ils utilisent le bouton Interaction par leur `KartInput`, comme un joueur. Leur comportement dépend de leur niveau (réglages par niveau, à régler) :

| Niveau | Coup de patte | Attrape | Se dégager |
|---|---|---|---|
| Facile | 10 % des occasions | jamais | saute après 1 s environ |
| Normal | 30 % des occasions | parfois, sur un kart plus rapide qui le dépasse (en boost) | saute après 0,5 s environ |
| Difficile | 60 % des occasions, de préférence pour pousser vers un muret, le hors-piste ou le vide | pour se faire remorquer par un kart plus rapide, puis le jette vers l'extérieur | saute après 0,25 s environ |

- Une **occasion**, c'est un kart qui entre à portée alors que la recharge est prête.
- Ils ne frappent pas pendant leur propre drift (ils gardent leur trajectoire), ne visent pas deux fois le même kart en moins de 5 s, et n'ont aucune préférence pour le joueur.
- Les interactions ne doivent pas dégrader leurs courses : les temps et l'absence de pélican mesurés sans interactions restent la référence.

## 16. Priorités

| Phase | Contenu |
|---|---|
| **1 – Conduite (solo)** | Conduite de base, caméra, circuit, drift et 3 paliers, départ fusée, figures, aspiration, panneaux, croquettes, dépanneur, course 3 tours avec classement, bots, statistiques et gabarits |
| **1b – Objets (solo)** | Boîtes, roulette, 2 emplacements, traîner, distribution par distance, objets de phase 1, réactions quand on est touché et invincibilité |
| **1c – Interactions (solo)** | Coup de patte et attrape entre karts (§ 15), effet du Poids, immunités, bots. Avant le réseau, pour que la synchronisation les prévoie dès le départ |
| **2 – En ligne** | Rooms P2P, synchronisation des karts, course synchronisée avec objets et croquettes, signalisation de prod. Plus les objets de phase 2. |
| **3 – Contenu** | Vrais modèles, personnages mous, Grand Prix et classes, anti-gravité, planeur, eau, objets de phase 3, mode Bataille |
