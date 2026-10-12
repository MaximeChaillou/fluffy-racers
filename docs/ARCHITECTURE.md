# Architecture

## Boucle de jeu

- `FixedUpdate` à **60 Hz** pour toute la simulation : entrées, physique, règles de course, réseau.
- `Update` pour le rendu : interpolation visuelle des transforms, caméra, UI, effets.
- Le flux de chaque tick : `lecture entrées → KartInput → physique kart → règles (checkpoints, objets) → envoi réseau`.

## Le kart (arcade, pas simulation)

On veut un kart **nerveux, lisible et indulgent**, pas réaliste.

- Corps rigide dynamique Rapier (collider boîte arrondie ou capsule), avec une masse et une inertie réglées pour ne pas se retourner facilement.
- **Suspension par 4 raycasts** (ressort + amortisseur) : le kart épouse le sol et rebondit de façon cartoon.
- Forces appliquées à la main :
  - **Moteur** : force le long de l'avant, avec une courbe d'accélération et une vitesse max.
  - **Grip latéral** : on annule une fraction de la vitesse de côté à chaque tick. Fort = rails, faible = glisse.
  - **Direction** : on pilote directement la vitesse angulaire en lacet, en fonction de la vitesse.
  - **Drift** : grip réduit, angle forcé, et charge d'un mini-turbo en 3 niveaux selon la durée.
  - **Boost** : impulsion et vitesse max temporairement augmentée.
  - **En l'air** : gravité renforcée pour des sauts courts, léger contrôle, et redressement automatique.
- Tous les paramètres vivent dans une `Resource` `KartTuning` documentée.

## Entrées

`KartInput { throttle: f32, brake: f32, steer: f32, drift: bool, use_item: bool }` est produit chaque tick par une source :
- clavier / manette (joueur local) ;
- bot (suit la ligne de course du circuit) ;
- réseau : on ne l'utilise pas pour les karts distants, voir plus bas.

## Réseau : P2P, sans serveur de jeu

Modèle inspiré des jeux de kart console : **chaque joueur fait autorité sur son propre kart.** C'est simple, ça donne zéro latence ressentie pour son propre kart, et ça tient jusqu'à 12 joueurs. Il n'y a pas d'anti-triche : c'est un jeu entre amis, c'est assumé.

### Topologie

- **Maillage complet WebRTC** via `matchbox_socket` : chaque navigateur est connecté à tous les autres (11 connexions max par joueur, que des petits paquets de données).
- Deux canaux :
  - **non fiable** : états des karts à haute fréquence ;
  - **fiable** : événements (lobby, départ, objets, touches, arrivée).
- **Hôte** = le pair au plus petit `PeerId`. Il arbitre l'état de la course : lobby, choix du circuit, compte à rebours et tick de départ, boîtes d'objets (qui a pris quoi), classement final. Si l'hôte part, le suivant prend le relais.

### Synchronisation

- **Horloge** : chaque pair estime son décalage avec l'hôte (ping/pong) pour parler en « tick de course ».
- **Mon kart** : simulé localement. Son état est diffusé à **30 Hz** : tick, position, rotation, vitesses, état de drift/boost/touché. Le tout est quantifié et compact (< 64 octets).
- **Karts distants** : affichés environ 100 ms dans le passé, par interpolation entre les snapshots reçus. On extrapole brièvement en cas de perte de paquets. Localement, ce sont des corps **kinématiques**.
- **Collisions kart contre kart** : chaque pair calcule le choc sur *son* kart contre le proxy kinématique de l'autre. Le rendu est approximativement symétrique ; le fun passe avant l'exactitude.
- **Objets** : le lanceur fait autorité sur la trajectoire. C'est **la victime qui décide** si elle est touchée, puis elle l'annonce. On évite ainsi le sentiment d'injustice dû à la latence.

### Mise en relation (signalisation)

WebRTC a besoin d'un petit service pour que les navigateurs se trouvent : il échange quelques Ko au moment de rejoindre, et **aucune donnée de jeu n'y transite**. Ce n'est pas un serveur de jeu.

- **Dev** : `matchbox_server` en local.
- **Prod** (issue dédiée) : une signalisation compatible avec le protocole matchbox sur **Cloudflare Workers + Durable Objects**, dans l'offre gratuite.
- **STUN** : serveurs publics. **TURN** : aucun au départ. Certains réseaux d'entreprise ou 4G peuvent bloquer la connexion P2P ; on traitera ce cas si besoin.
- **Rooms** : lien d'invitation `?room=CODE`.

## Personnages « mous »

Effet de mollesse procédural, sans vraie simulation de tissus :
- ressorts amortis sur la tête, les oreilles et la queue (os secondaires qui suivent avec du retard) ;
- squash & stretch aux atterrissages et aux chocs ;
- quand on est touché : mini-ragdoll de la tête et des bras pendant environ 1 s.

Tout ça est purement visuel. Ça ne touche jamais la physique du kart, donc ça n'a pas besoin d'être synchronisé en réseau.
