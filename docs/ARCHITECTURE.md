# Architecture

## Boucle de jeu

- `FixedUpdate` à **60 Hz** pour toute la simulation : entrées, physique, règles de course, réseau.
- `Update` pour le rendu : interpolation visuelle des transforms, caméra, UI, effets.
- Le flux de chaque tick : `lecture entrées → KartInput → physique kart → règles (checkpoints, objets, interactions) → envoi réseau`.

## Le kart (arcade, pas simulation)

Référence de ressenti : Mario Kart 8 Deluxe. Les règles et les valeurs sont dans `docs/GAME_DESIGN.md`. Ici, on décrit seulement *comment* on les implémente.

- **Machine à états** explicite : `Grounded`, `Hop`, `Drifting { side, charge }`, `Airborne`, `Trick`, `Hit { reaction, elapsed, duration }`, `Respawning`. Les transitions sont de la **logique pure**, testée unitairement. Dans l'état `Hit` (touché par un objet), le kart n'écoute plus son pilote et freine fort ; à la fin, il démarre son invincibilité (`Invincibility`, stat du § 9). Le tête-à-queue et la culbute ne sont que visuels : la carrosserie tourne ou se retourne, le corps simulé garde son cap.
- **Rapier** sert aux collisions (murs, karts, objets) et au contact avec le sol. Les vitesses viennent de nos courbes : chaque tick, on calcule la vitesse cible et on l'applique au corps. **La physique ne décide pas du ressenti.**
- Corps rigide dynamique (boîte arrondie ou capsule), avec une inertie réglée pour ne pas se retourner facilement. Son centre de masse est au milieu de la boîte (#27) : un muret ou un autre kart pousse sur un flanc de la boîte, à mi-hauteur, donc à travers le centre de masse, et le choc ne fait pas basculer le kart.
- **Suspension par 4 raycasts** (ressort + amortisseur) : le kart épouse le sol et rebondit de façon cartoon. Les rayons ne trouvent que le sol : un collider du circuit dont la `Surface` est du sol (route, hors-piste, tremplins), jamais un muret (`Surface::Wall`) ni un autre kart (#27).
- **Murets** : leur collider est plus haut que leur dessin (1,2 m au moins, contre 0,8 m dessinés) : un kart ne rencontre que leur face, jamais leur arête du haut (#27).
- **Moteur** : courbe d'accélération vers une vitesse max qui dépend de la classe, des stats, des croquettes, du terrain (hors-piste) et des boosts actifs.
- **Grip latéral** : on annule chaque tick une grande fraction de la vitesse de côté (sauf en drift).
- **Direction** : on pilote directement la vitesse angulaire en lacet, selon la vitesse et la stat Maniabilité.
- **Drift** (GD § 3) : la courbe du drift s'installe progressivement, à partir du lacet qu'avait le kart à l'atterrissage, et s'efface de même quand le drift se termine (`DriftMotion`, réglages dans `KartTuning`). L'angle de glisse suit un ressort amorti et reste visuel : le corps simulé suit la trajectoire (caméra, collisions et réseau inchangés), et la carrosserie pivote autour de l'essieu avant, ce qui fait glisser l'arrière vers l'extérieur. Un pair peut le recalculer pour un kart distant à partir de son état de drift et de son lacet.
- **Boosts** : un seul composant `Boost { strength, remaining }` pour toutes les sources (mini-turbo, piment, départ, figure, aspiration, panneau).
- **En l'air** : gravité renforcée pour des sauts courts, et redressement automatique.
- **Stats** (`docs/GAME_DESIGN.md` § 9) : `KartStats`, la somme des quatre éléments (personnage + kart + roues + aile, données RON de `assets/data/` compilées dans le jeu), calculée une fois au spawn. Elle est convertie au même moment en multiplicateurs (`StatEffects`, bornes dans la Resource `StatsTuning`), exactement 1 au niveau du milieu. À chaque tick, la conduite applique ces multiplicateurs à `KartTuning` pour ce kart (quelques multiplications, sans allocation) : un réglage modifié dans le panneau d'admin reste immédiat. La vitesse passe par `MaxSpeedFactors::stats`, comme les croquettes ; le Poids donne la masse du corps Rapier, ce qui décide qui pousse qui dans les chocs. Le niveau de Poids reste lisible pour les interactions entre karts (`KartStats::weight`, `strike_push_factor`, `tether_force_scale`).
- **Interactions entre karts** (coup de patte, attrape, `docs/GAME_DESIGN.md` § 15) : ce ne sont pas des états de la machine, puisqu'on frappe en drift. Un composant à part garde la recharge, la durée d'appui et le lien en cours. La conduite applique deux effets :
  - une **poussée latérale** qui s'éteint d'elle-même. Le grip annule presque toute vitesse de côté en un dixième de seconde, et le drift réoriente la vitesse : ni l'un ni l'autre ne doit l'effacer ;
  - l'**accélération du lien** élastique, un ressort amorti entre les deux karts.

  Pas de joint Rapier : la conduite réécrit la vitesse à chaque tick, et en ligne l'autre kart n'est qu'un proxy kinématique.

  Dans le code (#34) :
  - **le composant `Paw`** (`src/interactions/`) garde le bouton, la recharge, le lien côté attrapeur (`PawMotion::Holding`) ou côté cible (`Paw::held`), la protection d'1 s et l'état des visuels ;
  - **la logique pure** est dans `interactions/logic.rs` : `step_paw`, `reach`, `pick_target`, les immunités, `held_end`, `tether_accel` ;
  - **la conduite** applique deux composants du kart (`kart/push.rs`) :
    - `Shove` : la poussée, retirée de la vitesse avant la conduite puis rajoutée après ; ce qu'un muret ou un kart a arrêté y met fin ;
    - `TetherPull` : l'accélération du lien, et pour l'attrapeur la vitesse jusqu'à laquelle il est remorqué ;
  - **la cible tenue** est freinée par `MaxSpeedFactors::tether`.
  - Les systèmes (`InteractionSystems`) tournent en `FixedUpdate`, après les entrées, la porte de la course et les objets, avant les croquettes et la conduite.

## Objets (#16)

- **Logique pure** dans `src/items/` :
  - `table.rs` : table de distribution, redistribution des objets pas encore implémentés, tirage déterministe ;
  - `slots.rs` : les deux emplacements, la roulette, le bouton Objet ;
  - `objects.rs` : objets tenus autour du kart, pelote qui roule, contacts balayés.

  Les systèmes tournent en `FixedUpdate`, après les entrées et le verrou du départ, avant les croquettes et la conduite.
- **Aucun corps Rapier** pour les objets. Une pelote fait deux rayons par tick contre la piste (muret devant, sol dessous). L'abeille suit la ligne centrale (`Course`) puis plonge en ligne droite. Les contacts sont des segments (position au tick précédent → position actuelle) contre des sphères (objets) et la boîte de chaque kart : rien ne traverse entre deux ticks, et le coût reste de quelques dizaines de tests par tick avec 12 karts.
- **Position simulée et visuel** : un objet garde sa position simulée (`ItemObject::position` et `previous`) ; son `Transform` n'est que le visuel, interpolé entre deux ticks.
- **La victime décide** : un objet qui touche un kart casse et envoie `ItemHit { kart, by, item }`. Le système de la victime l'applique à son propre kart, ou l'ignore si le kart est immunisé (touché, pélican, invincible, arrivé). Il applique alors l'état `Hit`, la perte du boost et des objets tenus dehors, et la perte de croquettes (`LoseCroquettes`).

## Entrées

`KartInput { throttle: f32, brake: f32, steer: f32, jump_drift: bool, item: bool, look_back: bool, interact: bool }` est produit chaque tick par une source. `jump_drift` sert à la fois au saut, au drift et aux figures ; `item` maintenu permet de traîner un objet derrière soi, et un objet part derrière si `brake` ou `look_back` est tenu au moment du lancer (#16).

`look_back` ne sert qu'à la caméra (« regarder derrière », #17) et au lancer d'un objet derrière : le verrou du départ et la roue libre d'après l'arrivée le laissent passer. La caméra le lit sur le kart qu'elle suit, après la simulation ; elle passe devant le kart sans toucher aux ressorts de la caméra de poursuite, qui continuent de tourner.

`interact` est l'état du bouton Interaction (`docs/GAME_DESIGN.md` § 15). Les règles du kart en déduisent l'appui court (coup de patte, au relâchement) et le maintien (attraper), comme elles distinguent saut et drift sur `jump_drift`. Le côté visé quand il y a des karts des deux côtés, et le côté d'un jet, viennent de `steer`. Le verrou du départ met déjà tout le `KartInput` à zéro avant le GO ; le calage et l'arrivée ignorent aussi `interact`.

Les sources possibles :
- clavier / manette (joueur local). Les touches sont des positions physiques ; la disposition choisie dans les options (`KeyboardLayout`, AZERTY ou QWERTY, #17) donne les lettres affichées, et en AZERTY lit en plus les positions Z et Q. Les aides à la conduite (`docs/GAME_DESIGN.md` § 11) ne sont pas prévues pour l'instant ;
- bot (suit la ligne de course du circuit) ;
- réseau : on ne l'utilise pas pour les karts distants (voir plus bas).

Quand un champ de texte du panneau d'admin a le clavier, `ButtonInput<KeyCode>` est vidé à chaque frame (`admin::KeyboardCapture`) : aucune source ne voit alors de touche.

## Panneau d'admin et niveaux

- **Réglages** (`src/admin/`) : les Resources de réglage dérivent `Reflect` ; le panneau les parcourt par réflexion (`admin::tuning`), sans les connaître. Un champ ajouté apparaît tout seul ; une nouvelle Resource de réglage demande une ligne dans `AdminPlugin`. L'export ne contient que les valeurs modifiées, sous forme de texte RON (`"KartTuning.max_speed": 30.0`) ; c'est aussi ce qui est stocké dans le navigateur.
- **Coût** : l'interface egui (`bevy_egui`) n'existe que panneau ouvert. Le contexte egui est posé sur la caméra à l'ouverture et retiré à la fermeture ; sans contexte, egui ne fait rien.
- **Niveaux** (`src/level.rs`) : toute entité racine d'un niveau porte `LevelScoped` (pièces du circuit, karts, visuels, croquettes éparpillées, pélicans, HUD). `LoadLevel` les supprime, retire les Resources du niveau (`TrackLayout`, `Course`, `Race`, `RacingLine`…) et laisse le chemin habituel reconstruire : circuit, joueur, bots, course. La caméra reste et suit le nouveau kart. C'est ce qu'utiliseront aussi les menus et le Grand Prix (#19).

## Réseau : P2P, sans serveur de jeu

Modèle inspiré des jeux de kart console : **chaque joueur fait autorité sur son propre kart.** C'est simple, ça donne zéro latence ressentie pour son propre kart, et ça tient jusqu'à 12 joueurs. Il n'y a pas d'anti-triche : c'est un jeu entre amis, c'est assumé.

### Topologie

- **Maillage complet WebRTC** via `matchbox_socket` : chaque navigateur est connecté à tous les autres (11 connexions max par joueur, que des petits paquets de données).
- Deux canaux :
  - **non fiable** : états des karts à haute fréquence ;
  - **fiable** : événements (lobby, départ, objets, touches, interactions entre karts, arrivée).
- **Hôte** = le pair au plus petit `PeerId` parmi les joueurs assis (voir « Rooms et salon »). Il arbitre l'état de la course : lobby, choix du circuit, compte à rebours et tick de départ, boîtes d'objets (qui a pris quoi), classement final. Si l'hôte part, le suivant prend le relais.

### Rooms et salon (#7)

Le code est dans `src/net/`. Tout ce qui n'est pas le socket ou la fenêtre est de la logique pure, testée sans Bevy :

- **`room.rs`** : le code d'une room (5 caractères sans 0, O, 1, I, L), `?room=CODE&signal=…&pseudo=…` ou `--room CODE --signal … --pseudo …`, le lien d'invitation. La room s'appelle `fluffy-racers-CODE` sur le serveur de mise en relation (`ws://localhost:3536` par défaut).
- **`protocol.rs`** : un message par paquet, en petit-boutiste. Octet 0 : la version du protocole (`PROTOCOL_VERSION`) ; octet 1 : le type ; puis les champs. Le décodage ne panique jamais : un paquet vide, tronqué, trop long, d'un type inconnu, d'une autre version ou avec une valeur impossible (booléen à 2, flottant non fini) est refusé avec sa raison. Messages : `Hello` (nom, pièces du kart, prêt, assis), `Ready`, `Seats`, `RoomFull`, `Bye`, `StartRace` (graine, circuit, tours, bots, heure du compte à rebours sur l'horloge de l'hôte : envoyé par #9), les quatre événements d'interaction de #34 (`Strike`, `StrikeReply`, `Grab`, `TetherBreak`, conversions dans `net/interactions.rs`), `Ping` et `Pong`. Seuls les pings passent par le canal non fiable.
- **`lobby.rs`** : le salon vu d'un pair. Il lit les messages reçus et les pairs qui arrivent ou partent, et produit les messages à envoyer :
  - **places** : une room a 12 places. Les premiers pairs d'une room vide s'assoient d'eux-mêmes 2,5 s après leur arrivée, si aucun pair rencontré n'est assis (deux amis qui ouvrent le lien en même temps s'assoient tous les deux, puis se mettent d'accord). Ensuite, l'hôte assoit les nouveaux par ordre d'arrivée et envoie la liste des places (`Seats`) ; au-delà de 12, il répond `RoomFull` et le nouveau s'en va, avec un message ;
  - **hôte** : le plus petit `PeerId` parmi les joueurs assis. Chacun le calcule de son côté ; s'il part, le suivant prend le relais sans rien échanger. Un nouveau n'est jamais hôte avant d'être assis ;
  - **départs** : `Bye`, ou le socket qui signale le pair parti (onglet fermé : le serveur de mise en relation prévient les autres) ;
  - **ping et horloge** (`clock.rs`) : un ping par seconde vers chaque pair. Le pong rapporte l'horloge de celui qui répond ; on garde le décalage de l'échange le plus rapide parmi les 8 derniers. `Lobby::host_time_ms` donne l'heure de l'hôte : c'est l'horloge commune dont #9 tirera le tick de course ;
  - un pair d'une **autre version** est listé à part : il ne s'assoit pas, n'est jamais hôte et ne reçoit plus rien.
- **`socket.rs`** : `matchbox_socket` 0.14 utilisé directement, deux canaux (fiable puis non fiable). Sa boucle de messages tourne sur le pool de tâches d'E/S de Bevy, en natif comme dans le navigateur. Un système la sonde à chaque image. Deux pièges de la crate sont évités : sa boucle panique si on envoie à un pair dont les canaux ne sont pas encore ouverts (on n'envoie qu'aux pairs annoncés connectés), et un paquet peut arriver avant l'annonce (il attend). En quittant, seul le canal fiable est fermé : le `Bye` part, puis la boucle s'arrête d'elle-même.
- **`mod.rs`** : `NetPlugin`, avec `NetSession` (seulement quand on est dans une room) et les actions (`NetAction`) ; **`ui.rs`** : la fenêtre du salon (L) et la room demandée au démarrage. Sans room, pas de socket : seules la touche L et la lecture des actions tournent, le jeu solo est inchangé.

### Synchronisation

- **Horloge** : chaque pair estime son décalage avec l'hôte (ping/pong) pour parler en « tick de course ».
- **Mon kart** : simulé localement. Son état est diffusé à **30 Hz** : tick, position, rotation, vitesses, état de drift/boost/touché. Le tout est quantifié et compact (< 64 octets).
- **Karts distants** : affichés environ 100 ms dans le passé, par interpolation entre les snapshots reçus. On extrapole brièvement en cas de perte de paquets. Localement, ce sont des corps **kinématiques**.
- **Collisions kart contre kart** : chaque pair calcule le choc sur *son* kart contre le proxy kinématique de l'autre. Le rendu est approximativement symétrique ; le fun passe avant l'exactitude.
- **Bots** : l'hôte tire une graine de course et l'envoie avec le départ (`RaceGrid::seed`). Chaque pair en tire les mêmes bots : pièces du kart, donc stats et allure, et tirages de conduite (`bots::bot_seed`). Seul le pair qui simule un bot le conduit ; les autres le voient comme un kart distant.
- **Croquettes et boîtes à objets** : l'hôte décide qui a ramassé quoi (événement fiable). La roulette est tirée localement par le ramasseur, avec la table du game design.
- **Objets** : le lanceur fait autorité sur la trajectoire. C'est **la victime qui décide** si elle est touchée, puis elle l'annonce. On évite ainsi le sentiment d'injustice dû à la latence.
  - Le lanceur diffuse un événement fiable par objet lancé : objet, tick, position, vitesse, mouvement (pelote : rebonds restants ; abeille : cible et distance le long de la piste). Chaque pair simule ensuite l'objet lui-même. Le mouvement est déterministe contre une piste statique : rayons, ligne centrale, position de la cible.
  - Les objets tenus autour d'un kart (traîné, « x3 ») se déduisent de l'état de ses emplacements, diffusé avec le kart.
  - Le contact d'un objet avec **mon** kart est jugé chez moi : c'est l'entrée `ItemHit` de la victime. Je réponds « touché » ou « ignoré », comme pour un coup de patte.
  - Le tirage d'une boîte est déterministe : `draw_unit(graine de course, place de grille, nombre de tirages du kart)`. L'hôte n'arbitre que « qui a cassé la boîte ».

### Interactions entre karts

Un coup de patte ou une attrape (`docs/GAME_DESIGN.md` § 15) agit sur le kart d'un autre, alors que chacun fait autorité sur le sien. On suit la règle des objets : **la victime décide, et applique l'effet à son propre kart**. « La victime », c'est le pair qui fait autorité sur le kart visé : son joueur, ou le pair qui simule ce bot.

- **Événements**, sur le canal fiable, envoyés à tous les pairs :
  - `Coup { frappeur, cible, tick, tick_vu, position_du_frappeur, direction, jet }` : `direction` est la direction horizontale de la poussée, dans le repère du monde ; un jet est un coup avec `jet: true` ;
  - `CoupReçu { cible, frappeur }` ou `CoupIgnoré { cible, frappeur }` : la réponse de la victime ;
  - `Attrape { attrapeur, cible, tick, tick_vu, position_de_l_attrapeur }` ;
  - `FinAttrape { attrapeur, cible, raison }`. Raisons : lâcher, jet, dégagé, durée écoulée, lien trop étiré, touché, pélican, arrivée, refus, déconnexion.
- **Qui fait autorité** :
  - **le frappeur** décide de sa cible (d'après ce qu'il voit), de sa recharge et de son coup dans le vide. Chez lui, la patte et le « pof » partent tout de suite : c'est un visuel prédit. Il ne déplace jamais le proxy de la cible ; l'écart arrive avec les états de la victime ;
  - **la victime** décide si le coup la touche (immunités, protection d'1 s, portée vérifiée comme ci-dessous). Elle applique à son kart la poussée, la perte de vitesse et la croquette, au tick de réception, puis répond `CoupReçu`. Cette réponse déclenche chez tous les pairs le ragdoll, le « pof » et l'éparpillement de la croquette. Comme les autres croquettes éparpillées, celle-ci est déterministe, et l'hôte arbitre son ramassage.
- **Latence et portée** : les karts distants sont affichés environ 100 ms dans le passé, soit 2,8 m derrière leur vraie place à 28 m/s. Le frappeur juge la portée **sur son écran**, contre le proxy qu'il voit : ce que le joueur voit toucher touche. Il envoie `tick_vu`, le tick de l'état de la cible qu'il affichait, avec sa propre position au tick du coup.
  - La victime garde un historique de ses propres positions : 30 ticks (0,5 s), dans un tampon circulaire, sans allocation.
  - Elle compare sa position à `tick_vu` avec la position du frappeur : à portée, plus 0,75 m de tolérance (à régler), elle applique le coup. Sinon, elle répond `CoupIgnoré` : le frappeur a vu sa patte frapper sans effet, comme un objet qui rate.
  - Un événement vieux de plus de 0,3 s (à régler) est ignoré.
  - Ce contrôle sert la cohérence (la cible a été ramassée par le pélican entre-temps, par exemple), pas l'anti-triche.
- **Attrape** :
  - l'attrapeur commence à se faire remorquer tout de suite (prédiction) et envoie `Attrape`. La cible vérifie comme pour un coup ; si elle refuse (immunisée, hors de portée), elle répond `FinAttrape { raison: refus }`, et l'attrapeur lâche avec le déséquilibre de la griffe dans le vide ;
  - **chaque pair applique la force du lien à son propre kart**, vers la position de l'autre **extrapolée au tick courant** (dernier état reçu + vitesse × retard), pas vers le proxy affiché 100 ms dans le passé. Sinon, à pleine vitesse, chacun verrait l'autre 2,8 m trop en arrière : le lien tirerait trop peu l'attrapeur et freinerait trop la cible ;
  - les deux pairs calculent la même formule (ressort amorti ; les stats de chaque kart sont connues de tous depuis le lobby), donc des forces à peu près opposées. Le fun passe avant l'exactitude, comme pour les chocs ;
  - **fin du lien** : chacun la décide pour son côté et l'annonce par `FinAttrape`. L'attrapeur lâche, jette ou saute ; la cible se dégage, est touchée ou part avec le pélican ; l'un ou l'autre constate que le lien est trop étiré ou qu'il a franchi l'arrivée. Chacun applique aussi la **durée maximale de 2 s** à partir du tick de la prise : un lien ne dure jamais plus, même si un message tarde ;
  - un jet est un `Coup` (`jet: true`) envoyé juste après la `FinAttrape` ; la victime le traite comme un coup.
- **Déconnexion pendant une attrape** : le pair restant rompt le lien dès que `matchbox_socket` signale le départ de l'autre, ou après 0,5 s sans aucun état reçu de lui (à régler), et au plus tard au bout des 2 s. Pour lui, c'est un lâcher sans jet, sans pénalité ni effet. Un bot dont le simulateur change (l'hôte part) perd aussi son lien. Le reste (le kart du joueur parti, sa place au classement) relève de la course en ligne (#9).
- **Rien de plus dans l'état diffusé à 30 Hz** : les autres pairs dessinent la patte, le bras élastique et le ragdoll à partir des événements. Le ragdoll reste purement visuel (voir « Personnages mous »).
- **Dans le code (#34)**, ces événements sont déjà des messages Bevy ; en solo, tout passe par eux, sur le même tick :
  - `Coup` est `KartStruck { striker, target, tick, seen, striker_position, direction, push, throw }`. `push` est la vitesse de poussée, Poids et jet compris ;
  - `CoupReçu` et `CoupIgnoré` sont `StrikeAnswer { landed }` ;
  - `Attrape` est `KartGrabbed { grabber, target, tick, seen, grabber_position, rest }` ;
  - `FinAttrape` est `TetherEnded { reason: TetherEnd }`, avec un refus (`Refused`) ou un départ (`Gone`).

  La victime vérifie l'événement avec `accepts` : portée plus 0,75 m, au plus 0,3 s d'âge. Elle prend aujourd'hui sa position actuelle ; l'historique de 30 ticks viendra avec #8.

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
- quand on est touché ou qu'on reçoit un coup de patte : mini-ragdoll de la tête et des bras pendant environ 1 s ;
- la patte ou le bras qui frappe ou attrape (`docs/GAME_DESIGN.md` § 15) : animation procédurale. La patte se lève à l'appui puis frappe ; pendant une attrape, un bras élastique est tendu entre les deux karts, et il revient en « boing » au lâcher. La portée et le lien sont décidés par les règles du kart, jamais par la longueur du bras affiché.

Tout ça est purement visuel. Ça ne touche jamais la physique du kart, donc ça n'a pas besoin d'être synchronisé en réseau : en ligne, les événements d'interaction suffisent à le déclencher chez chaque pair.
