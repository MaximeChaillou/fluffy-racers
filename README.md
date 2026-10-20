# Fluffy Racers

Jeu de kart multijoueur dans le navigateur, avec des animaux tout mous. Rust + Bevy, compilé en WebAssembly.

**Jouer :** https://maximechaillou.github.io/fluffy-racers/ (la dernière version de `dev`, mise à jour automatiquement)

Branches : `dev` reçoit automatiquement le travail de Claude (fusionné quand la CI passe), `main` est la version stable.

## Développer

Prérequis : Rust stable (installé via [rustup](https://rustup.rs)) et [trunk](https://trunkrs.dev).

```bash
cargo run      # version native
trunk serve    # version web sur http://localhost:8080
cargo test
```

Sur Linux, il faut aussi : `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.

## Jouer à plusieurs (dev)

Celui qui **crée la partie** en est l'**hôte** : chaque autre joueur se connecte directement à lui, et seulement à lui (WebRTC, une « étoile » autour de l'hôte, #63) ; l'hôte relaie aux autres ce que chacun dit, et envoie à chacun la position de tous les karts. Le « serveur », c'est le navigateur de l'hôte. Pour se trouver, les joueurs passent par un petit **serveur de mise en relation** : il n'échange que quelques Ko au moment de rejoindre, aucune donnée de jeu n'y passe. En attendant celui de production (issue #10), on lance `matchbox_server` sur sa machine. **Si l'hôte s'en va, la partie s'arrête** pour tout le monde (« L'hôte a quitté la partie »).

On se retrouve dans le **salon** (touche **L**, il s'ouvre tout seul en arrivant et à la fin de chaque course) : la liste des joueurs, le lien de chacun avec l'hôte (« connecté, 12 ms », « en cours… », « échec »), l'hôte, et l'action principale en gros : **Lancer la course** chez l'hôte (grisé tant qu'il manque quelqu'un, avec la raison : « Bob n'est pas prêt. »), **Prêt !** chez les autres. En haut de l'écran, **un bandeau** rappelle hors course où l'on est et quoi faire (#64) : « Salon ABCDE · 3 joueurs · Tu es l'hôte : choisis le circuit puis lance la course (L) », ou « … · En attente de l'hôte (Ana)… » ; il disparaît en course. Une fois assis dans la partie, **tout le monde roule librement sur le circuit choisi par l'hôte** et **chacun voit les karts des autres**. Quand tout le monde est prêt, **l'hôte lance la course** (#9) : même circuit, même grille (les bots de l'hôte devant, les joueurs derrière), compte à rebours commun, classement partagé et mêmes résultats sur tous les écrans. **En ligne, rien ne met le jeu en pause.** Les **objets et les croquettes** sont partagés (#53) : l'hôte décide qui a pris une boîte ou une croquette, le lanceur d'un objet fait autorité sur sa trajectoire, et c'est la fenêtre du joueur touché qui décide s'il est touché.

### 1. Installer et lancer le serveur de mise en relation

```bash
cargo install matchbox_server --version 0.14.0   # une seule fois (environ 2 min)
matchbox_server                                  # écoute sur le port 3536 ; laisse-le tourner
```

### 2. Deux fenêtres sur ton ordinateur

```bash
trunk serve          # dans un autre terminal : le jeu sur http://localhost:8080
```

1. Dans Chrome, choisis **Multijoueur** sur l'accueil (ou appuie sur **L**), puis **Créer une partie** : tu es l'**hôte** ; l'adresse devient `http://localhost:8080/?room=CODE&host=1` (« host=1 » : c'est toi qui héberges ; un rechargement héberge de nouveau la même partie), et le salon montre le **lien d'invitation**, sans « host=1 » (bouton **Copier le lien**). Ou bien ouvre directement http://localhost:8080/?room=TEST&host=1.
2. Ouvre le **lien d'invitation** (`http://localhost:8080/?room=CODE`, **sans** `host=1`) dans une **nouvelle fenêtre** Chrome, et mets les deux fenêtres côte à côte : le navigateur ralentit beaucoup un onglet caché (le jeu y tourne au ralenti, son ping monte et il peut s'afficher « muet »). Sans hôte dans la partie (le lien ouvert avant que l'hôte l'ait créée, ou l'hôte parti), le salon dit au bout de 5 s « Cette partie n'existe plus », avec **Créer une partie**. Un lien avec `host=1` ouvert une seconde fois est refusé : « La partie a déjà un hôte », avec **La rejoindre**.
3. Le lien ouvert, l'écran **Pilote** s'ouvre d'abord (ton animal, ton kart, ton pseudo) ; ferme-le (**Échap**, ou **Entrée** pour garder ton choix) : le salon apparaît. En une ou deux secondes, chaque fenêtre montre les deux joueurs : le nom (le pseudo tapé dans l'écran **Pilote**, G, ligne « Pseudo », retenu d'une fois sur l'autre ; `&pseudo=Max` dans l'adresse passe devant ; sinon celui de l'animal), le lien avec l'hôte (« connecté, 3 ms »), « hôte » sur celui qui a créé la partie. En haut de l'écran, le bandeau : « Salon CODE · 2 joueurs · … ». En bas du salon, dans **Détails de connexion** (replié) : « Connexion directe : sans serveur STUN » (inutile en local).
4. Dans le terminal de `matchbox_server` : deux lignes `Client connected …`, puis une ligne `… -> "{\"NewPeer\":…}"` (le serveur présente le nouveau à ceux qui sont là ; seul l'hôte lui fait une offre). **Le serveur n'écrit jamais les offres ni les réponses** (`Offer`, `Answer`, `IceCandidate`), même quand tout marche : c'est la console du navigateur qui les montre.
5. Dans la console du navigateur (F12, onglet Console), les étapes de la connexion : chez l'hôte, `network: joining room CODE … as its host`, puis `network: #f599 announced by the signaling server: we make the offer`, `network: offer sent to #f599 after 0.01 s: 1 candidate (1 host .local)`, `network: answer received from #f599…` ; chez l'autre, `… as its client`, `network: offer received…` et `network: answer sent…` ; puis des deux côtés `network: #… connected, 0.05 s after it was announced…`. Un troisième joueur : les clients déjà là écrivent `network: #… entered the room: the host connects it` (ils ne se connectent jamais entre eux).
6. Clique sur **Prêt !** dans la fenêtre du client : « prêt » s'affiche aussi chez l'hôte, son bandeau dit « En attente de l'hôte (…)… », et chez l'hôte, **Lancer la course** n'est plus grisé (l'hôte n'a pas de bouton « Prêt » : lancer la course, c'est être prêt). Ferme l'onglet d'un client : il disparaît du salon des autres (tout de suite avec **Quitter la partie**). **Ferme celui de l'hôte** : la fenêtre de l'autre affiche « L'hôte a quitté la partie » (la course s'arrête, retour à l'accueil depuis #89), avec **Rejoindre de nouveau** (si l'hôte a rechargé sa page, ça le retrouve) et **Créer une partie**.
7. **Rouler ensemble** (#8) : dès qu'il est assis dans la partie (l'hôte tout de suite, chacun dès que l'hôte l'a placé), chaque fenêtre passe sur **Prairie Pompon en roulage libre** (le circuit de l'hôte ; la console écrit `network: free driving on circuit 0, from grid slot N`). Si l'écran **Pilote** est encore ouvert, ferme-le (**Entrée**) dans les deux fenêtres. Chaque kart part de la place de la grille de son siège : l'hôte en pole, les autres à côté, dans leur ordre d'arrivée. Ce que tu dois voir :
   - roule dans une fenêtre (flèches, ou Z/Q/S/D) : dans l'autre, ton kart avance **en douceur**, environ un dixième de seconde en retard, avec ton animal et la couleur que tu as choisie (point 10) ;
   - drift (Espace) : l'autre fenêtre montre la glisse et les étincelles bleues, oranges, violettes ; un boost (B, ou un mini-turbo) montre les flammes ; R te ramène sur ta place de la grille, et l'autre fenêtre te voit **sauter** jusqu'à elle, sans glisser ;
   - fonce dans l'autre kart : **ton** kart est repoussé, l'autre ne bouge pas sur ton écran (c'est son joueur qui le conduit) ; sur l'autre écran, c'est son kart qui est repoussé par le tien. Pas d'éjection, pas de kart qui monte sur l'autre ;
   - **Quitter la partie** : tu reviens à l'accueil (#89 ; avant, au circuit et au mode d'avant) ;
   - la console (F12) écrit toutes les 3 s où est ton kart (`network: our kart at (x, y, z)`) et où tu vois chacun des autres (`network: kart 1 of #abcd at …`, avec l'id du kart et le nombre d'états reçus) : de quoi comparer les deux fenêtres.
8. **La course en ligne** (#9) :
   - **le circuit** : dans le salon de l'hôte (« hôte » à côté de son nom), la liste **Circuit** : choisis « Anneau Guimauve » ; les deux fenêtres passent dessus en roulage libre (« Roulage libre sur Anneau Guimauve » dans le salon). Remets « Prairie Pompon ». Chez l'autre joueur, le salon dit « L'hôte (…) choisit le circuit et lance la course. » et « En attendant la course, roule librement sur le circuit avec les autres. » ;
   - **le départ** : clique sur **Prêt !** dans la fenêtre de l'autre joueur, puis **Lancer la course** dans celle de l'hôte (grisé tant qu'un joueur n'est pas prêt, la raison écrite dessous). Le salon et le bandeau disparaissent, et les deux fenêtres passent sur la grille en même temps : 10 bots devant, puis les deux joueurs côte à côte (11e et 12e places, dans leur ordre d'arrivée dans la partie) ; 2 s plus tard, la grille puis « 3, 2, 1, GO » **au même moment dans les deux fenêtres** (la console écrit `network: race 1 on circuit 0, 10 bots, from grid slot N, starting at tick T`, le même T des deux côtés). Le départ fusée marche comme en solo ;
   - **les bots** : ce sont ceux de l'hôte : dans l'autre fenêtre, les mêmes animaux aux mêmes places, qui roulent pareil (un dixième de seconde en retard) ;
   - **la course** : le classement en haut à gauche compte les 12 coureurs, les deux joueurs compris ; il peut différer d'une place entre les deux fenêtres quand deux karts sont au coude à coude (chacun voit l'autre un dixième de seconde dans le passé). Tu peux te coller dans le sillage de l'autre joueur (aspiration), lui donner un **coup de patte** (X ou F) ou **l'attraper** (tenir X) : c'est **sa** fenêtre qui décide si le coup porte, puis le « pof » s'affiche des deux côtés ;
   - **l'arrivée** : quand les deux joueurs ont fini (ou 30 s après le premier), l'hôte termine la course : « FINI ! », puis **le même classement dans les deux fenêtres**, avec les temps, ton nom (« Toi ») et celui de l'autre. La console de l'hôte écrit `network: the race is over at race tick …`. Le salon se rouvre tout seul, avec le bandeau : pour la course suivante, **Lancer la course** dans celui de l'hôte (ou un autre circuit) ;
   - **pas de pause** : ouvre le menu d'options (**O**) ou l'écran **Pilote** (**G**) en pleine course : le jeu continue, ton kart roule en roue libre et l'autre fenêtre le voit avancer. Un nouveau kart choisi dans **Pilote** sert à la course suivante ;
   - **un joueur s'en va** en pleine course (un client) : son kart est classé dernier, la course continue ; **l'hôte s'en va** : la course s'arrête pour tous (« L'hôte a quitté la partie »), personne ne reprend les bots ;
   - un joueur qui arrive pendant une course roule librement à côté et la regarde ; il sera de la suivante (son bandeau : « Course en cours : tu seras de la prochaine »).
9. **Les objets et les croquettes en ligne** (#53), en course (ou déjà en roulage libre) :
   - **les boîtes** : roule dans une boîte (la première ligne est sur la ligne droite de départ) : ta roulette tourne tout de suite (case en haut à gauche), et **la boîte disparaît aussi dans l'autre fenêtre**, puis revient 2 s plus tard au même moment dans les deux. Les deux karts dans la même boîte au même instant : un seul garde son objet (l'autre voit sa roulette s'arrêter, vide). La console des deux fenêtres écrit toutes les 3 s ce qui s'est passé (`network: items: 4 boxes and 6 croquettes taken, 3 items out, 1 hits, 1 croquette losses`) : les mêmes nombres des deux côtés, à un objet en route près ;
   - **lancer** (E ou Shift ; avec Bas/S ou C : derrière) : une **pelote** sur l'autre joueur part de ton kart et roule **au même endroit dans l'autre fenêtre** ; c'est **sa** fenêtre qui décide s'il est touché : s'il l'est, il fait une culbute dans les deux fenêtres, perd 3 croquettes (elles s'éparpillent au même endroit des deux côtés) puis clignote (invincible) ; touché pendant qu'il clignote, la pelote casse sans rien lui faire. Dans **ta** fenêtre, la pelote peut traverser son kart un dixième de seconde avant de disparaître : c'est le temps que sa fenêtre te réponde ;
   - **l'abeille** : derrière l'autre joueur, lance-la : elle file vers le kart juste devant toi au classement (le même dans les deux fenêtres) et lui fait un tête-à-queue ;
   - **les boucliers** : tiens E avec une pelure ou une pelote : elle reste derrière ton kart, **visible dans l'autre fenêtre** ; un objet lancé par l'autre sur ton bouclier casse avec lui, et ton kart n'est pas touché. Un « x3 » sorti tourne autour de ton kart, des deux côtés ;
   - **les croquettes** : ramasse celles de la piste : elles disparaissent aussi dans l'autre fenêtre (et reviennent 10 s plus tard, au même moment) ; celles qu'un kart perd (choc, coup de patte, objet) s'éparpillent au même endroit dans les deux fenêtres, et une croquette ramassée par l'un disparaît chez l'autre ;
   - **le piment et la croquette dorée** : l'autre fenêtre voit les flammes de ton boost ;
   - **les objets de phase 2 en ligne** (#18, #61) : dans une fenêtre, **I** met un objet dans ta main (le suivant à chaque appui ; en ligne, seulement avec `trunk serve` ou `cargo run` ; tous les objets) :
     - **la bombe** (boule bleu marine, mèche jaune) : lancée devant l'autre joueur, elle part en cloche et explose là où elle retombe, au même moment dans les deux fenêtres ; s'il est à moins de 6 m, il fait une culbute (c'est **sa** fenêtre qui décide). Posée derrière toi (avec Bas/S), elle explose sur l'autre s'il roule dessus, au même endroit dans les deux fenêtres, ou au bout de 3 s ;
     - **le klaxon-canard** : les objets à moins de 9 m de ton kart disparaissent dans les deux fenêtres (une pelure posée par l'autre, ce qu'il traîne derrière lui), et l'autre fenêtre voit l'onde orange ;
     - **le piment doré** : 7 s de boosts, un par appui ; l'autre fenêtre voit tes flammes ;
     - **la chantilly** (#61) : derrière l'autre joueur, utilise-la : **sa** fenêtre se couvre de crème 4 s (pas la tienne) ; devant lui, rien ;
     - **le dégonflage** (#61) : dans la fenêtre de l'autre, son kart (et les bots, si c'est l'hôte) rapetisse, ralentit et perd ses objets, d'autant plus longtemps qu'il est bien classé **dans ton classement** ; ta fenêtre le voit petit ;
     - **le super câlin** et **la fusée** (#61) : l'autre fenêtre voit ton halo rose ou ta coque orange ; fonce dans son kart : c'est **sa** fenêtre qui décide qu'il fait une culbute ;
     - **l'oreiller géant** (#61) : il vole vers le 1er de **ton** classement, le même dans les deux fenêtres, et l'écrase dans la sienne ; l'autre peut le détruire avec un klaxon-canard pendant qu'il plane au-dessus de lui. Les boîtes n'en donnent qu'un à la fois, pas avant 20 s de course ni dans les 30 s après le précédent : c'est l'hôte qui décide ; refusé, ta roulette s'arrête sur un autre objet ;
     - la console écrit ce qui est passé (`… 1 oreillers granted, 5 items out (2 bombes), 1 klaxons, 2 effects, 1 rams …`), les mêmes nombres des deux côtés ;
   - **les objets de phase 3 en ligne** (#21), avec **I** comme ci-dessus :
     - **le boomerang-os** (un os crème qui tourne à plat) : lancé, il part en tournant du côté où tu braques (tout droit : vers sa gauche) et revient vers toi, dans les deux fenêtres ; tu le rattrapes, puis le lancer suivant (3 en tout). Sur l'autre kart, c'est **sa** fenêtre qui décide le tête-à-queue ;
     - **la piñata** : ses 8 objets tournent autour de ton kart, **aussi dans l'autre fenêtre** ; chaque appui utilise celui qui est devant, il disparaît des deux côtés ;
     - **la pie voleuse** : quand l'autre joueur tient un objet, utilise-la : **sa** case d'objet se vide et l'objet arrive dans la tienne (avec des bots qui en tiennent aussi, la victime est tirée au hasard, le 1er le plus souvent) ; 3 s durant, ton kart est un fantôme (à peine visible dans sa fenêtre), et ses objets te traversent ;
   - **les bots de l'hôte** prennent des boîtes et lancent leurs objets comme en solo, dans les deux fenêtres ;
   - **un client s'en va** : les objets qu'il avait lancés disparaissent chez tous.
10. **Les couleurs** (#55) : dans chaque fenêtre, **G** (**Pilote**), descends sur la dernière ligne, **Couleur** (Bas depuis l'aile), choisis avec Gauche / Droite (ou clique sur une pastille), puis **Entrée** :
    - deux couleurs différentes : chaque fenêtre montre ton kart dans ta couleur et celui de l'autre dans la sienne ; la colonne **Couleur** du salon montre la pastille et le nom de chacun ;
    - **la même couleur** des deux côtés : le premier arrivé dans la partie la garde, l'autre prend la couleur libre la plus proche (rouge → orange, bleu → violet, rose → violet…), **la même dans les deux fenêtres** ; son salon écrit « Couleur déjà prise : tu es en orange au lieu de rouge. », et la colonne **Couleur** montre « (rouge pris) » à côté de sa pastille. Si le premier quitte la partie, l'autre retrouve sa couleur ;
    - **en course**, les bots gardent leurs couleurs, sauf celui qui porterait la couleur d'un joueur : il en prend une libre, la même dans les deux fenêtres ;
    - la couleur est gardée : recharge la page, elle est toujours là (et en solo, le bot qui avait ta couleur en change aussi).
11. Même chose avec **Chrome + Safari** : ouvre la même adresse dans Safari. Pour voir sa console : Safari > Réglages > Avancés > « Afficher les fonctionnalités pour les développeurs web », puis Développement > Afficher la console JavaScript.

Si la connexion ne se fait pas, le salon le dit (« Connexion à l'hôte en cours… », puis « n'aboutit pas » avec les pistes sous **Dépannage**) : voir **Dépannage** plus bas.

### 3. Jouer sur le réseau local (plusieurs Mac)

Le test de Max à 3 Mac, **sans toucher au réglage Chrome** (la mDNS) : seuls comptent les liens entre l'hôte et chacun des autres.

1. **Sur le Mac de l'hôte** : trouve son adresse IP, `ipconfig getifaddr en0` (Wi-Fi ; si rien ne s'affiche, `en1`), par exemple `192.168.1.20`. Lance `matchbox_server`, puis `trunk serve` : le jeu écoute sur toutes les interfaces (`Trunk.toml`), et trunk affiche ses adresses au démarrage.
2. **Sur ce même Mac**, ouvre `http://192.168.1.20:8080/` (son IP, pas `localhost`), **L**, **Créer une partie** : il est l'hôte (l'adresse devient `http://192.168.1.20:8080/?room=CODE&host=1`). **Copier le lien** donne `http://192.168.1.20:8080/?room=CODE`.
3. **Sur les autres Mac**, ouvre ce lien : `http://192.168.1.20:8080/?room=CODE` (**sans** `host=1`). La page trouve toute seule le serveur de mise en relation sur le Mac de l'hôte (`ws://192.168.1.20:3536`, affiché dans le salon), sans `&signal=`.
4. Si macOS demande d'**accepter les connexions entrantes** (pour `matchbox_server` ou `trunk`) ou si le navigateur veut **accéder aux appareils du réseau local**, accepte.
5. Ce que tu dois voir : chaque salon liste les 3 joueurs, **dans le même ordre**, l'hôte en premier, chacun « connecté » (avec son ping chez l'hôte, et chez chacun celui de son lien avec l'hôte) ; dans la console de l'hôte, `offer sent` / `answer received` puis `connected` pour chacun ; dans celle d'un client, `network: #… entered the room: the host connects it` quand un autre arrive. Puis Prairie Pompon sur les 3 Mac : **chaque Mac voit les 3 karts**, roule sur l'un, ton kart roule sur les deux autres ; et la course du point 8 : **Prêt !** chez les clients, **Lancer la course** chez l'hôte, avec les objets et les croquettes du point 9.
6. Si un Mac reste à « en cours… » ou « échec » dans la colonne **Lien**, c'est son lien avec l'hôte qui ne s'ouvre pas : les pistes sont dans le salon et dans **Dépannage**. Les autres ne sont pas gênés.

Ouverte en `localhost`, la page ne connaît pas l'adresse de ton Mac sur le réseau (le salon le rappelle) : pour inviter quelqu'un, ouvre-la avec l'adresse IP.

### 4. Par Internet

Il faut un serveur de mise en relation public en `wss://` (issue #10, en attente) ; `?signal=wss://…` le désigne. Le lien d'invitation garde ce paramètre.

Par Internet, chaque navigateur demande son adresse publique aux serveurs STUN publics de Google. Sans relais TURN, certains réseaux (entreprise, 4G) bloquent la connexion directe ; un relais se donne avec `?turn=turn:relais.exemple.org:3478&turnuser=NOM&turnpass=MOT_DE_PASSE` (celui de production viendra avec #10).

### Simuler un mauvais réseau (debug)

Pour voir comment les karts des autres se comportent sur une mauvaise connexion, le jeu peut ajouter de la latence, de la gigue et des pertes aux états des karts qu'il **reçoit** (pas au salon ni aux pings : le ping affiché reste le vrai) :

- dans l'adresse : `http://localhost:8080/?room=TEST&netsim=120,20,5` : 120 ms de latence, ± 20 ms de gigue, 5 % de pertes (`&netsim=80` : la latence seule) ;
- ou pendant le jeu, dans le panneau d'admin (**P**), section « Avancé » : `NetTuning` > `simulator` (`latency_ms`, `jitter_ms`, `loss_percent`), effet immédiat. Remets-les à 0 (ou le bouton « défaut (tout) » de `NetTuning`) pour l'arrêter : le panneau garde les réglages modifiés d'une session à l'autre. Attention : avec `?netsim=` dans l'adresse, si tu modifies un autre réglage dans le panneau, le simulateur est mémorisé avec ;
- chaque fenêtre simule son côté : pour ralentir les deux sens, mets `netsim` dans les deux adresses.

Ce que tu dois voir avec `netsim=120,20,5` : le kart de l'autre roule toujours **sans à-coups**, simplement un peu plus en retard (environ 0,2 s au lieu de 0,1 s). Les réglages de l'affichage (délai de 100 ms, extrapolation de 250 ms au plus…) sont aussi dans `NetTuning`.

### Réglages de la connexion

| Page | Natif | Effet |
|---|---|---|
| `?host=1` | `--host` | Héberger la partie (#63 : celui qui la crée ; « Créer une partie » le met dans l'adresse de l'hôte seulement, jamais dans le lien d'invitation). |
| `?signal=ws://…` | `--signal ws://…` | Le serveur de mise en relation. Par défaut : celui de la machine qui sert la page quand elle est locale, sinon `ws://localhost:3536`. |
| `?stun=none` | `--stun none` | Aucun serveur STUN (le défaut quand la mise en relation est locale). |
| `?stun=stun:hôte:port,…` | `--stun …` | Ces serveurs STUN. `?stun=auto` : le défaut. |
| `?turn=turn:hôte:port&turnuser=…&turnpass=…` | `--turn … --turnuser … --turnpass …` | Un relais TURN (les trois sont nécessaires). Aucun par défaut. |
| `?netsim=120,20,5` | `--netsim 120,20,5` | Simulateur de réseau (debug) : latence (ms), gigue (ms), pertes (%) des états de karts reçus. |

Une valeur illisible est ignorée, avec un avertissement dans la console.

### Dépannage

**Le salon dit « La connexion avec l'hôte n'aboutit pas »** (chez l'hôte : « avec N joueurs »), ou « échec » dans la colonne **Lien**, ou chacun reste seul (« 1 joueur sur 12 »). Depuis #63, seul compte le lien de chacun avec l'hôte : c'est celui-là qui n'aboutit pas. Les pistes sont aussi dans le salon, sous **Dépannage** (replié). Pistes, dans l'ordre :

1. **macOS** : Réglages Système > Confidentialité et sécurité > **Réseau local** : active ton navigateur (Chrome, Safari…), puis quitte-le et relance-le. Sans cette autorisation, le navigateur ne peut pas annoncer ses adresses locales (mDNS).
2. **Chrome** : ouvre `chrome://flags/#enable-webrtc-hide-local-ips-with-mdns` (« Anonymize local IPs exposed by WebRTC »), passe-le à **Disabled**, puis relance Chrome. C'est ce qui a débloqué Max (#49) : Chrome cache ses adresses locales derrière des noms `….local`, et sur son Mac ces noms ne marchaient pas.
3. **Un réseau qui bloque l'UDP** (entreprise) : il faudra un relais TURN (`?turn=…`, prévu avec #10).

Autres causes possibles : une extension anti-fuite WebRTC (uBlock Origin « Empêcher WebRTC de divulguer les adresses IP locales », WebRTC Leak Prevent…), un VPN, une politique Chrome (`chrome://policy`, `WebRtcIPHandlingPolicy`). Pour les écarter : essaie en navigation privée (sans extensions).

**La console** (F12) écrit chaque étape : `announced`, `offer sent … N candidates`, `answer received …`, `connected`. `no candidate` dans une offre ou une réponse : ce navigateur n'a trouvé aucune adresse où l'autre peut le joindre (pistes 1 et 2). `[fluffy-net] the direct connection failed (ICE failed)` : les deux navigateurs ont essayé toutes leurs adresses, sans réponse. Les lignes `matchbox_socket` (en `DEBUG`) donnent le détail de la crate (« created offer », « Ice gathering completed »…).

**`chrome://webrtc-internals`** : ouvre-le dans un onglet **avant** de rejoindre la partie, puis rejoins-la dans un autre onglet. Chaque connexion a sa ligne (l'adresse du jeu) ; clique dessus et regarde :

- la configuration (`iceServers`) ;
- « ICE connection state » et « Connection state » (`new => checking => connected`, ou `failed`) ;
- la **grille des candidats** (« ICE candidate grid ») : **vide, c'est qu'aucun candidat n'a été trouvé** (pistes 1 et 2) ;
- les événements `setLocalDescription` / `setRemoteDescription` et `icecandidate`.

À envoyer si ça ne marche toujours pas : ces quatre éléments (une capture suffit), ou le fichier de **Create a WebRTC-Internals dump** (en haut de la page), avec la console du jeu et le terminal de `matchbox_server`.

### Sur la version en ligne (GitHub Pages)

La page est en `https`, et le serveur par défaut est `ws://localhost:3536` (non chiffré). Chrome l'accepte quand même pour `localhost` ; Firefox aussi en principe ; Safari le refuse sans doute (contenu mixte). Le plus sûr pour tester : `trunk serve` en local, comme plus haut.

### En natif

```bash
cargo run -- --room CODE --host          # crée (héberge) la partie CODE (L ouvre le salon)
cargo run -- --room CODE                 # rejoint la partie CODE, en client
cargo run -- --room CODE --pseudo Max --signal ws://192.168.1.20:3536
cargo run -- --room CODE --stun none     # sans STUN (le défaut avec une mise en relation locale)
```

Un test automatique fait se rencontrer deux clients natifs (serveur lancé ; `FLUFFY_STUN=auto` par défaut, `FLUFFY_STUN=stun:stun.l.google.com:19302` pour passer par un STUN) :

```bash
cargo test --lib net::scenario_tests::two_native_peers -- --ignored --nocapture
```

Un autre (#63) : l'hôte et trois clients, chacun relié à l'hôte seulement, la même liste partout, un second « hôte » refusé, puis l'hôte qui s'en va :

```bash
cargo test --lib net::scenario_tests::a_host_and_three -- --ignored --nocapture
```

Un autre fait rouler deux jeux complets (sans fenêtre) dans la même partie : Ana roule, et le test compare, tick par tick, où Bob voit son kart et où il était (`FLUFFY_NETSIM=120,20,5` met le simulateur chez Bob) :

```bash
cargo test --lib net::scenario_tests::two_games_drive -- --ignored --nocapture
```

Un troisième fait courir deux jeux complets l'un contre l'autre (#9) : l'hôte lance une course de 3 tours avec 10 bots, les deux joueurs sont conduits par des bots jusqu'à l'arrivée, et le test vérifie que les deux jeux s'accordent (circuit, tick de départ, bots, GO, tours, arrivées, classement final) :

```bash
cargo test --lib net::race_tests::two_games_race -- --ignored --nocapture
```

Deux pages Chromium qui se connectent pour de vrai (Node et Playwright, `matchbox_server` et `trunk serve` lancés ; la première héberge, `--pages 4` en ouvre quatre) : `scripts/webrtc-pages.cjs` (voir son en-tête).

La bande passante de l'hôte avec 12 joueurs (12 jeux complets en mémoire, #63) :

```bash
cargo test --lib net::star_tests::the_hosts_bandwidth -- --ignored --nocapture
```

## Comment ce projet est construit

Le code est écrit par Claude Code, la plupart du temps en tâche de fond :

- [`CLAUDE.md`](CLAUDE.md) : règles, stack figée, conventions ;
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) : kart, réseau P2P, personnages ;
- [`STATUS.md`](STATUS.md) : où on en est, décisions, questions ouvertes ;
- les **issues GitHub** forment le backlog : une issue = une PR.

## Crédits

Voir [`assets/CREDITS.md`](assets/CREDITS.md).
