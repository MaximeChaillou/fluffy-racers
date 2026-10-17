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
| Championnat (Grand Prix) | 4 courses pour une coupe (de 2 au nombre de circuits pour un championnat personnalisé, #99) ; points du 1er au 12e : **15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1** (MK8) |
| Vitesse | **Une seule**, celle des réglages du jeu (§ 2) : pas de classes de vitesse (décision de Max du 2026-10-17, #118). Plus un mode miroir |
| Départ | Grille en quinconce, compte à rebours « 3, 2, 1, GO » d'une seconde par étape |
| Modes | **Jouer solo** (#99) : Championnat (une coupe ou un championnat personnalisé), Contre-la-montre (3 piments, fantôme ; voir plus bas), Circuit seul (l'ancienne course libre). En ligne (jusqu'à 12) : l'hôte choisit. **Pas de mode Bataille** (décision de Max du 2026-10-16, #21) |
| Réglages de partie (#99) | Le **miroir**, **Bots : oui / non** et **Objets : oui / non**, en solo comme en ligne. Pas de classe de vitesse (#118) |

### Jouer solo et réglages de partie (#99)

Demande de Max (2026-10-17) : « Dans solo on a le choix de championnat, contre la montre, circuit seul. Le championnat peut être une coupe déjà définie ou un championnat custom avec un certain nombre de circuits définis ou aléatoire. Je veux qu'on ait la possibilité en solo et en multi de faire aussi ces choix sur le championnat, ainsi que la possibilité d'activer ou désactiver les bots et les objets. »

- **Trois modes**, dans cet ordre : **Championnat** (ci-dessous), **Contre-la-montre** (le circuit et le miroir), **Circuit seul** (un circuit, une course).
- **Le championnat** est :
  - **une coupe** : quatre circuits de `assets/tracks/index.ron`, dans son ordre (la 1re coupe court les quatre premiers, la 2e les quatre suivants…), chacune avec un nom original et son icône (le manifeste : « Coupe Doudou », « Coupe Nuage »…). **Une coupe qui n'a pas encore ses quatre circuits n'est pas proposée** (notre choix : une coupe a quatre courses ; ses circuits se courent seuls ou dans un championnat personnalisé). Avec 6 circuits aujourd'hui, il y en a une ; avec 24 à terme, six ;
  - **ou un championnat personnalisé** : un **nombre de courses** (de 2 au nombre de circuits), puis ses circuits **au choix** (un par un, courus dans l'ordre choisi ; un circuit peut revenir) ou **au hasard** : tirés de la graine de la course (`RaceGrid::seed`) et du numéro du tirage, **toujours les mêmes pour la même graine et le même tirage, et jamais deux fois le même circuit tant qu'il en reste** ; l'écran montre le tirage avant le départ, et chaque nouveau nombre de courses, chaque retour à « Au hasard » et chaque championnat lancé fait un nouveau tirage.
  - Les points, les égalités, la grille inversée, les mêmes adversaires et le podium sont ceux de la suite (« Grand Prix (#19) »), quelles que soient ses courses. Ses écrans disent son nom : celui de la coupe (« COUPE DOUDOU 2/4 »), ou « CHAMPIONNAT ».
- **Bots : oui** : ils complètent la grille jusqu'à 12, au niveau choisi (panneau d'admin), comme avant. **Bots : non** : seuls les joueurs courent ; en solo, une course seul, sans fantôme, avec les objets si on les veut (un championnat aussi : le podium n'a que le joueur). Le nombre de bots du joueur est gardé et revient avec « Bots : oui ».
- **Objets : non** : **aucune boîte à objets sur la piste** (donc aucun objet tiré au hasard) ; les croquettes restent. Le contre-la-montre n'en a jamais et garde ses 3 piments.
- **Le nombre de tours** reste 3 (`RaceTuning::laps`). Les **circuits « ouverts »** (une longue course sans tours, une par coupe : #119, plus tard) prendront place dans le manifeste et dans les coupes comme les autres.
- **En ligne** : l'hôte choisit, dans le salon, le circuit, le **miroir**, les **bots** et les **objets** ; chaque joueur voit ses choix, roule librement avec eux et court la course avec eux (sans bots : seuls les joueurs sur la grille). Le **championnat en ligne** (l'hôte porte le classement général et la grille) vient avec #122 : en ligne, on court un circuit à la fois.
- Apparence et navigation : `docs/UI_CHARTE.md` § « L'écran Jouer solo (#99) ».

### Grand Prix (#19)

Le championnat de #99 (« Championnat » dans les écrans) ; ses règles :

- **La coupe** : les quatre premiers circuits de `assets/tracks/index.ron`, dans cet ordre (Prairie Pompon, Anneau Guimauve, Crique Câline, Col des Moufles), dans le mode miroir choisi pour tout le Grand Prix. Depuis #99, une coupe du manifeste, ou les courses d'un championnat personnalisé.
- **Points** : ceux du tableau ci-dessus, d'après le classement final de chaque course (le tableau clos de #70 : un coureur encore en piste prend sa place estimée).
- **Égalités** (non tranché par MK8DX dans nos sources ; notre règle) : à points égaux, le mieux classé de la **dernière course** passe devant, puis celui de la course d'avant, et ainsi de suite. Tous les coureurs ont une place à chaque course : il n'y a donc jamais d'égalité au bout du compte.
- **Grille de la course suivante** (comme MK8DX) : l'ordre inverse du classement général, le leader en dernière position. La première course part comme une course libre : les bots devant, le joueur derrière.
- **Les mêmes adversaires** : chaque bot garde son animal, ses pièces, sa couleur et son nom d'une course à l'autre, quelle que soit sa place sur la grille.
- **Entre les courses** : le classement de la course, puis le classement général (points de la course et total) ; Entrée lance la suivante.
- **Podium** : après la dernière course, les trois premiers sur leurs marches, qui dansent, caméra fixe ; Entrée relance le même championnat.
- **Refaire une course** (un nouveau kart choisi avec G, « Relancer » du panneau d'admin) : la course du Grand Prix repart, ses points sont repris.
- **En solo pour l'instant**. En ligne, le championnat viendra avec #122 (l'hôte devra porter le classement et la grille).

### Miroir (#19)

- **Pas de classes de vitesse** (décision de Max du 2026-10-17, #118 : « seule la 150cc m'intéresse ») : le jeu se joue à une seule vitesse, celle de ses réglages (§ 2), qui était le 150cc de #19. Les classes 50cc, 100cc et 200cc et le frein en drift du 200cc ont été retirés.
- **Miroir** : le circuit est inversé gauche-droite (tracé, dévers, bonus sur la route, sable, décor, grille : la pole passe de l'autre côté). Les modèles du décor ne sont pas retournés.
- **En ligne** (#99) : l'hôte choisit le miroir dans le salon ; il vaut pour la course et pour le roulage libre en attendant.

### Contre-la-montre (#76)

- **Les mêmes choix que le circuit seul** : le circuit et le miroir (« Jouer solo », N). Pas de bots ni d'objets à choisir : il est seul, avec ses 3 piments. Pas en ligne : entrer dans une partie termine le contre-la-montre.
- **Seul** : aucun bot, aucune boîte à objets (donc aucun objet tiré au hasard) et, sans personne, aucune interaction. Les croquettes restent (comme MK8DX).
- **3 piments** (à régler) : un « x3 » en main au départ de chaque course, un piment par appui, et rien d'autre.
- **Le chrono** : le temps total et le temps de chaque tour. Le **record** est gardé par circuit et selon le miroir, dans le navigateur (#118 : les records gardés avant, qui l'étaient aussi par classe, restent lisibles : ceux du 150cc deviennent ceux de leur circuit, ceux des autres classes sont ignorés) ; il s'affiche au départ et à l'arrivée, avec « RECORD ! » quand il est battu. Un temps égal ne bat pas le record.
- **Le fantôme** : la course du record, rejouée par un kart translucide, avec l'animal, les pièces et la couleur de cette course. Il **ne touche rien** : pas de collision, pas de cible pour les objets, et **pas d'aspiration** derrière lui (notre choix, comme les fantômes du contre-la-montre de MK8DX). Il disparaît une fois la ligne passée. Le fantôme rejoue les positions enregistrées, pas les commandes : il reste juste quand le jeu change de version.

## 2. Conduite de base

C'est de l'arcade : le kart est piloté par une **machine à états** (au sol, saut, drift, boost, en l'air, figure, touché, dépannage) et des **courbes de réglage**. La physique Rapier gère les collisions et le contact avec le sol, mais **ne décide pas du ressenti**.

- **Vitesse max** : 26 m/s, soit environ 94 km/h, pour un kart de 2 m (à régler). Accélération de 0 à la vitesse max en environ 2,5 s pour un kart moyen (à régler, dépend de la stat Accélération).
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
   - en direction assistée (« auto-drift ») : 2 et 0,8 unités/tick (MK8DX). Les aides ne sont pas prévues pour l'instant (§ 11) : ces valeurs restent dans les réglages, inutilisées en jeu.
4. **Paliers et étincelles** (MK8DX) :

| Palier | Seuil (à régler, modulé par la stat MT) | Durée au plus vite | Étincelles | Durée du boost (MK8DX) |
|---|---|---|---|---|
| Mini-turbo | 270 | ≈ 0,9 s | **bleues** | **0,62 s** |
| Super mini-turbo | 570 | ≈ 1,9 s | **orange** | **1,67 s** |
| Ultra mini-turbo | 870 | ≈ 2,9 s | **violettes** | **2,63 s** |

   Les seuils 270 et 570 sont ceux de MK8 sur Wii U. Dans MK8DX, ils dépendent de la stat MT ; 870 est notre extrapolation pour le 3e palier.
5. **Relâcher le bouton** déclenche le boost du palier atteint. Relâcher avant le premier palier ne donne rien.
6. L'ultra mini-turbo est **désactivé quand la direction assistée est active** (MK8DX). Sans objet tant que les aides ne sont pas prévues (§ 11).
7. **Freiner pendant un drift** le casse : le kart passe sous la vitesse minimale du drift (#118 : le frein du 200cc de #19, qui le resserrait sans le casser, est parti avec les classes de vitesse).

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
- **Touché** : **tête-à-queue** (≈ 1 s, objets légers) ou **culbute** (≈ 1,6 s, explosions et gros objets). Ensuite, une **invincibilité** de 1 à 2 s, qui dépend de la stat Invincibilité (MK8DX : 1,5 s environ, mesuré dans son mode bataille).

### Notre implémentation (phase 1, #16)

Toutes les valeurs sont dans la Resource `ItemTuning` (panneau d'admin), la table de distribution dans `assets/data/items.ron`.

- **Boîtes** : un kart qui touche une boîte la casse et tire un objet par emplacement libre (deux pour une double boîte). Les deux emplacements pris, la boîte casse sans rien donner (MK8DX). Elle revient 2 s plus tard. Un kart transporté par le pélican ou arrivé ne prend rien.
- **Tirage** : décidé au moment de la boîte, selon la distance au leader à cet instant ; déterministe (graine de course, place de grille, nombre de tirages du kart).
- **Utiliser** (bouton Objet) : à l'appui. Une **pelure** ou une **pelote** seule sort derrière le kart tant qu'on maintient le bouton, et part au relâchement. Une fois sortie, elle libère son emplacement : la réserve passe en main et on peut prendre une boîte en la traînant (MK8DX). Un **« x3 »** de pelures, pelotes ou abeilles sort au premier appui : les pelures en file derrière le kart, les pelotes et les abeilles en tournant autour. Il reste en main, et chaque appui suivant en lâche ou en lance une. Un **piment x3**, c'est trois appuis, trois boosts.
- **Lancer derrière** : Objet avec la direction arrière, Bas/S (le frein) ou C (regarder derrière). À la manette, B ou X/Y. Une pelure est toujours posée derrière : le clavier n'a pas de touche « vers l'avant » libre, puisque Haut accélère.
- **Pelure** : posée au sol derrière le kart. Celui qui roule dessus fait un tête-à-queue. Elle disparaît après 90 s.
- **Pelote** : roule tout droit à 42 m/s, plus vite qu'un kart en boost. Elle rebondit 5 fois sur les murets, casse au suivant et disparaît après 8 s.
- **Abeille** : elle vise le kart **juste devant** son lanceur au classement. Elle suit la ligne centrale de la piste au-dessus des karts, à 50 m/s, puis plonge sur sa cible à moins de 12 m. Le leader n'a pas de cible : son abeille plonge sur le premier kart qu'elle approche. Lancée derrière, elle roule tout droit et casse sur le premier muret.
- **Abeille = tête-à-queue** : c'est un objet léger (piqûre). La culbute reste pour la pelote, objet lourd.
- **Bouclier** : deux objets de karts différents qui se touchent cassent tous les deux. Une pelure ou une pelote traînée (ou un « x3 » sorti) arrête donc le premier objet qui l'atteint : projectile, ou pelure au sol. Les objets d'un même kart ne se touchent jamais. Sa propre pelote ou pelure peut toucher son lanceur 0,5 s après être partie ; sa propre abeille, jamais.
- **Touché** : la victime décide (voir `docs/ARCHITECTURE.md`). Elle ne prend rien pendant un tête-à-queue ou une culbute, avec le pélican, après l'arrivée, ni pendant son invincibilité. Sinon :
  - le kart freine fort sans écouter le pilote ;
  - son drift, son boost et ce qu'il tenait dehors sont perdus ;
  - il perd 3 croquettes (§ 7).

  L'invincibilité (1 à 2 s selon la stat) commence à la fin du tête-à-queue ou de la culbute ; le kart clignote. Un objet qui touche un kart immunisé casse quand même.
- **Piment** : boost de 1,2 s, +30 %. **Croquette dorée** : +2 croquettes (10 au plus), avec le mini-boost d'une croquette.
- **Bots** : ils font un léger écart vers une boîte proche sur les lignes droites quand un emplacement est libre. Ils utilisent leurs objets avec un délai qui dépend de leur niveau (`BotSkill::item_delay`) :
  - le piment en ligne droite ;
  - la croquette dorée dès qu'elle sert ;
  - la pelure ou la pelote traînée en bouclier, puis lâchée ;
  - la pelote sur un kart en ligne de mire, ou derrière sur un kart qui suit de près ;
  - l'abeille dès qu'un kart est devant au classement.
- **Sans objets** : `ItemTuning::enabled` les retire de la course (ni boîtes, ni objets). Les tests de conduite des bots tournent ainsi.

### Notre implémentation (phase 2, #18)

Toutes les valeurs sont dans `ItemTuning` (panneau d'admin) ; celles des bots dans `BotTuning`. Toute la table est en jeu : chaque objet a la part que lui donne le tableau ci-dessous.

- **Piment doré** : le premier appui ouvre une fenêtre de 7 s ; chaque appui pendant ce temps est le boost d'un piment (+30 %, 1,2 s). Il reste en main jusqu'à la fin de la fenêtre (la réserve attend), puis il est épuisé.
- **Bombe à confettis** : lancée devant, elle part en cloche (20 m/s de plus que le kart, 7 m/s vers le haut) et explose là où elle retombe, environ 17 m devant son lanceur : hors de sa propre explosion. Lancée derrière (Bas/S ou C), elle est posée au sol et explose au bout de 3 s. Dans les deux cas, elle explose aussi sur le premier kart ou objet qu'elle touche. **Explosion** : tout kart à moins de 6 m fait une culbute ; ce qu'il traîne ne le protège pas.
- **Chantilly** : les coureurs devant son lanceur au classement (hors course : devant lui) ont de la crème sur l'écran pendant 4 s ; elle s'efface pendant la dernière seconde. Un bot crémé conduit moins précisément : l'amplitude de son flottement autour de sa ligne grandit de 2,5 m (`BotTuning::creamed_wobble`).
- **Super câlin** : 7 s d'invincibilité (le kart ne clignote pas : un halo rose), +15 % de vitesse max ; tout kart qu'il touche fait une culbute.
- **Fusée à confettis** : pendant 6 s, un pilote automatique conduit le kart à 42 m/s le long de la ligne de course des bots, sans écouter son pilote ; le kart est invincible et renverse (culbute) ceux qu'il touche. Il ne s'envole pas sur les tremplins. À l'arrivée, il s'arrête.
- **Dégonflage** : tous les autres karts rapetissent (taille × 0,6) et roulent moins vite (vitesse max × 0,8) ; leurs objets tombent (les deux emplacements, la roulette, ce qu'ils tiennent dehors). Durée : 7 s pour le leader, 3 s pour le dernier, entre les deux selon le rang.
- **Oreiller géant** : il vole à 60 m/s, 4 m au-dessus de la piste, vers le **leader du moment** (même si le leader change en route, et même si c'est son lanceur), plane 0,8 s au-dessus de lui, puis tombe : culbute pour tout kart à moins de 3,5 m. Seul le klaxon-canard l'arrête (les objets traînés, non). **Règles** : une boîte ne le donne pas avant 20 s de course, ni tant qu'un autre existe (dans un emplacement, une roulette ou en vol), ni moins de 30 s après le tirage du précédent ; son poids va alors aux autres objets de la ligne.
- **Klaxon-canard** : tous les objets à moins de 9 m de son lanceur, sauf les siens, sont détruits : posés, lancés, traînés ou tournant autour d'un autre kart, l'oreiller géant compris. Il ne touche pas les karts.
- **Réactions** : la bombe, l'oreiller géant, le super câlin et la fusée font une **culbute** ; la victime décide, comme pour tout objet.
- **Invincibilité et immunités** :
  - ce qui touche ou explose (bombe, oreiller géant, karts en super câlin ou en fusée) épargne un kart déjà touché, invincible (après un objet, en super câlin, en fusée ; depuis #21, invisible après une pie voleuse), avec le pélican ou arrivé ; l'objet casse quand même (sauf sur un kart invisible : il le traverse) ;
  - la chantilly et le dégonflage épargnent un kart invincible, avec le pélican ou arrivé, mais pas un kart en tête-à-queue ou en culbute ;
  - le super câlin et la fusée rendent invincible : ni objet, ni coup de patte, ni attrape (§ 15) ;
  - le piment doré ne protège de rien (un boost) ;
  - le klaxon-canard ne s'occupe que des objets.
- **Bots** (`BotSkill::item_delay` avant tout objet) : le piment doré en ligne droite, puis un boost après l'autre ; la bombe sur un kart devant entre 7 et 24 m, ou posée devant un kart qui suit ; la chantilly, le dégonflage et l'oreiller géant dès qu'un kart est devant au classement (le leader garde son oreiller) ; le super câlin avec un kart à moins de 12 m ou en ligne droite ; la fusée dès qu'elle est prête ; le klaxon-canard quand l'objet d'un autre arrive à moins de 9 m.
- **En ligne** (#18 puis #61) : tous les objets, avec la table solo. La victime décide toujours pour son kart. La chantilly et le dégonflage suivent le classement de leur lanceur au moment où il les utilise (le même rang pour un kart sur tous les écrans). Les règles de l'oreiller géant (pas avant 20 s, un seul à la fois, 30 s entre deux) sont celles de l'hôte, sur l'horloge commune : un oreiller refusé devient le même tirage sans lui, comme en solo ; l'oreiller vise le leader du classement de son lanceur.
- **Debug** : la touche **I** met dans la main du joueur l'objet suivant de la liste (tous les objets, « x3 » compris), prêt à servir ; le panneau d'admin (P) a un menu « Donner un objet ». En ligne, seulement dans une version de développement (`cargo run`, `trunk serve`), et seulement les objets qui marchent en ligne (tous depuis #61).

### Notre implémentation (phase 3, #21)

Toutes les valeurs sont dans `ItemTuning` (panneau d'admin) ; la table de distribution dans `assets/data/items.ron` (voir plus bas : leurs parts sont les nôtres, le game design n'en donnait pas).

- **Pie voleuse** : invisible pendant 3 s. Son joueur voit à la place de son kart un fantôme blanc translucide, avec deux yeux ; les autres n'en voient presque rien (le même fantôme, à peine visible). Pendant ce temps :
  - les objets la **traversent** (aucun contact), rien ne la vise (l'abeille, l'oreiller géant, les bots), aucune patte ne l'atteint ;
  - elle est invincible (sans clignoter) : explosions, chantilly, dégonflage et karts qui renversent l'épargnent aussi ;
  - elle peut toujours prendre des boîtes et utiliser ses objets.

  Au moment où elle sert, elle **vole l'objet** d'un kart **tiré au hasard** parmi ceux qui en ont un à prendre, devant ou derrière elle, **avec une probabilité d'autant plus forte qu'il est devant au classement** (décision de Max, #88) : le poids d'un kart est `(N − rang + 1)^k` (N coureurs, le leader au rang 1), avec **k = 2** (à régler, `pie_rank_power`). Le leader est donc le plus probable : avec 12 karts, 144 pour le 1er, 121 pour le 2e, 100 pour le 3e… 1 pour le dernier (k = 1 : 12, 11… 1 ; k = 0 : tous pareils). Le tirage est déterministe (graine de course, place de grille de la voleuse, nombre d'objets qu'elle a utilisés), comme la roulette. Hors course, le kart le plus proche qui en a un. On prend l'objet prêt dans sa main (un « x3 » ou une piñata sortis restent autour de lui, comme un piment doré dont la fenêtre est ouverte), sinon celui de sa réserve ; jamais une roulette qui tourne. L'objet volé arrive prêt dans un emplacement libre de la voleuse. Si personne n'a rien, elle ne vole rien (elle reste invisible).
- **Piñata** : au premier appui, ses **8 objets** sortent et tournent autour du kart, plus loin et plus lentement qu'un « x3 » (2,4 m, 2,5 rad/s) : pelure, pelote, abeille, bombe, piment, chantilly, super câlin et croquette dorée. Ils protègent comme un « x3 » (un objet qui les touche casse avec l'un d'eux) ; tant qu'elle tourne, sa bombe n'explose pas (elle casse comme un bouclier, ou renverse le kart qu'elle touche), sinon elle renverserait son propre kart. Chaque appui suivant utilise **celui qui est devant** le kart, comme s'il était seul : la pelure posée là où elle tournait, la pelote lancée, l'abeille, la bombe, le boost du piment, la chantilly, le super câlin, les deux croquettes. Touché, le kart perd ceux qui restent.
- **Boomerang-os** : **3 lancers**, un par appui. Lancé devant (ou derrière, avec Bas/S ou C), il file au ras du sol à 42 m/s **en tournant du côté où son lanceur braque au moment du lancer** (décision de Max, #88 : « il tourne avec le kart ») : à fond, 1,5 rad/s (à régler : un rayon de 28 m, environ 60° sur ses 30 m), moins avec moins de braquage ; **tout droit** (braquage sous 0,15 sur 1, à régler), vers sa gauche à 0,6 rad/s (notre choix, à régler : la courbe de #21). Lancé derrière, braquer à gauche le fait tourner vers sa gauche à lui (la gauche de l'image avec la caméra arrière). La courbe est figée au lancer. Il file ainsi sur 30 m, puis **revient** vers son lanceur (en virant au plus de 6 rad/s, droit sur lui à moins de 8 m), qui le rattrape à 2 m : le lancer suivant est prêt à ce moment-là, pas avant. Le premier muret à l'aller le renvoie ; au retour, il rebondit sur 2 murets et casse au troisième. Il casse sur le premier kart qu'il touche (**tête-à-queue**, un objet léger) et le lancer suivant est prêt aussitôt ; il ne touche jamais son lanceur. Il disparaît au bout de 5 s s'il n'est pas revenu.
- **Bots** : la pie voleuse dès qu'un kart est devant au classement (le leader au bout d'un moment : il vole derrière lui) ; la piñata tout de suite, puis un appui par seconde environ ; le boomerang-os comme une pelote : sur un kart dans leur ligne de mire, derrière sur un kart qui suit de près, devant au bout d'un moment ; lancé devant sur un kart, **ils braquent vers lui au moment du lancer** (#88) : la courbe qui le rejoint là où il sera, s'il roule à leur vitesse. Ils ne visent pas un kart invisible.
- **En ligne** (#21) : les trois, avec la table solo.
  - Le **boomerang-os** part comme les autres objets : son lanceur fait autorité sur sa trajectoire, sa courbe comprise (#88 : elle part avec l'objet lancé, la même trajectoire sur tous les écrans). Au retour, chaque écran l'envoie vers le kart du lanceur tel qu'il le montre, et c'est le lanceur qui dit quand il l'a rattrapé : il disparaît alors partout. La victime décide toujours si elle est touchée.
  - La **piñata** sortie voyage avec l'état du kart (les objets qui restent) : tout le monde la voit tourner.
  - La **pie voleuse** : « invisible » voyage avec l'état du kart (son fantôme pâle chez les autres, son invincibilité). Le vol est une demande au pair de la victime : **le pair de la voleuse la tire** (#88), sur son classement, parmi les karts dont l'état dit « il a un objet à prendre », et l'annonce ; le pair de la victime donne ce que son kart tient à ce moment-là, s'il tient encore quelque chose, et le dit à tous : le pair de la voleuse le met dans son emplacement.

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

Les objets de phase 3 (#21) n'avaient pas de part : la pie voleuse et le boomerang-os sont au milieu du peloton, la piñata loin derrière, comme leurs équivalents dans MK8DX. Leur part est prise sur les objets les plus fréquents de la même ligne (abeilles, piments, pelures), et chaque ligne fait toujours 100 %.

| Distance au leader | Objets probables |
|---|---|
| leader / < 15 m | Pelure 30 %, Pelote 25 %, Croquette dorée 20 %, Pelure x3 10 %, Klaxon-canard 10 %, Bombe 5 % |
| 15 – 50 m | Pelote 20 %, Abeille 20 %, Pelure 10 %, Piment 15 %, Bombe 10 %, Pelote x3 10 %, Chantilly 5 %, Boomerang-os 5 %, Pie voleuse 5 % |
| 50 – 120 m | Abeille 20 %, Abeille x3 15 %, Piment 15 %, Piment x3 10 %, Chantilly 5 %, Oreiller géant 5 %, Pelote x3 10 %, Boomerang-os 10 %, Pie voleuse 5 %, Piñata 5 % |
| 120 – 250 m | Piment x3 20 %, Abeille x3 15 %, Super câlin 15 %, Piment doré 15 %, Oreiller géant 5 %, Fusée 10 %, Dégonflage 5 %, Piñata 10 %, Pie voleuse 5 % |
| > 250 m | Super câlin 20 %, Fusée 25 %, Piment doré 20 %, Piment x3 10 %, Dégonflage 10 %, Oreiller géant 5 %, Piñata 10 % |

La table est une donnée de configuration (RON), pas du code. Tant qu'un objet n'est pas implémenté, on le retire de la table et on redistribue son poids proportionnellement sur les autres lignes.

## 9. Statistiques des personnages et karts

Modèle MK8DX :

- **Stats** : Vitesse, Accélération, Poids, Maniabilité, Adhérence (hors-piste et surfaces glissantes), **Mini-turbo** et **Invincibilité** (stats cachées). La vitesse et la maniabilité existent en 4 variantes de terrain (sol, eau, air, anti-gravité) : chaque pièce donne ses points sur chaque terrain (#20 ; une valeur omise reprend celle du sol), le kart prend les niveaux du terrain où il est (sous l'eau : « eau » ; planeur ouvert : « air » ; l'anti-gravité viendra avec #78). Les bornes de leurs effets sont celles du sol par défaut (`StatsTuning::water`, `air`, `anti_gravity`), exactement 1 au niveau 10. La combinaison par défaut fait 10 sur chaque terrain. Quelques pièces ont un terrain de prédilection : la baignoire et le pingouin nagent, l'éléphant aussi (sa trompe) ; la plume et le perroquet planent ; le panier en osier prend l'eau.
- **Somme de 4 éléments** : personnage + kart + roues + aile. Chaque élément vaut de 0 à 10 points par stat. Le total est borné à un **niveau de 0 à 20**. Le menu affiche `(niveau + 3) / 4`, soit de 0,75 à 5,75.
- **L'écran « Pilote »** (#98, § 13) montre ces stats en barres, à côté du kart vu de 3/4 face avec son pilote dedans : les sept stats (les deux cachées de MK8DX comprises), puis la Vitesse et la Maniabilité sur chaque terrain (sol, eau, air, anti-gravité). Tout suit le choix à l'instant où il change.
- **Effets** : chaque niveau est converti en paramètres par interpolation linéaire. Exemple : vitesse max = base × lerp(0,92 ; 1,08 ; niveau / 20) (à régler).
  - **Poids** : qui pousse qui dans les collisions, et la force des interactions entre karts (§ 15) : un lourd frappe et jette plus fort, et freine davantage le kart qu'il attrape ; un léger se fait remorquer sans presque freiner sa cible.
  - **Maniabilité** : vivacité de la direction et rayon de drift.
  - **Mini-turbo** : seuils de charge, puis durée et force des boosts de drift et de figure.
  - **Invincibilité** : durée de la protection après avoir été touché par un objet (§ 8).
- **Trois gabarits**, avec le compromis classique (MK8DX) : les légers ont plus d'accélération et de maniabilité, les lourds plus de vitesse et de poids.

Roster de départ (original, 3 par gabarit ; validé par Max le 2026-10-15, #69 : les animaux du pack CC0 Cube Pets de Kenney) :

| Gabarit | Animaux |
|---|---|
| Léger | poussin, pingouin, perroquet |
| Moyen | chat, lapin, renard |
| Lourd | cochon, vache, éléphant |

Dans chaque gabarit, le compromis reste celui du dessus ; l'éléphant est le plus lourd de tous, le poussin le plus léger. Un pilote est animé (#69) : il s'agite doucement en course et sur la grille, secoue la tête quand il est touché (objet, coup de patte), danse à l'arrivée s'il est sur le podium et hoche la tête de contentement sinon.

Pièces de départ (#15, données dans `assets/data/`) : 3 karts (panier en osier, caisse à savon, baignoire), 3 jeux de roues (bobines de fil, pneus guimauve, pneus de tracteur) et 2 ailes (taie d'oreiller, plume). Chaque pièce se voit sur le kart (#73) : une baignoire est une baignoire, des bobines de fil sont des bobines, l'aile s'ouvre en taie d'oreiller ou en plume. Le personnage moyen « chat » avec la caisse à savon, les pneus guimauve et la taie d'oreiller fait exactement le niveau du milieu (10) partout.

## 10. Environnements spéciaux (phase 3)

- **Anti-gravité** : zones marquées où les roues pivotent et où l'on roule sur les murs et les plafonds. **Se percuter donne un boost aux deux karts** (spin boost, MK8DX). Des plots dédiés donnent aussi un boost.
- **Planeur** : un tremplin spécial ouvre une aile ; on dirige alors haut, bas, gauche et droite avec les stats « air ».
- **Sous l'eau** : on passe aux stats « eau », plus lentes et plus flottantes.

### Notre implémentation (#20 : le planeur et l'eau ; l'anti-gravité : #78)

Toutes les valeurs sont dans `KartTuning::environment` (`EnvironmentTuning` : panneau d'admin, à régler).

- **Planeur** :
  - un **tremplin de planeur** (bleu ciel, chevrons blancs), marqué dans le RON du circuit (`glide_ramps`), large d'un muret à l'autre, mène à un **vide** (`gaps` : un tronçon sans route ni murets, que la ligne centrale traverse toujours). Une figure y marche aussi (c'est un tremplin à figures) ;
  - l'aile s'ouvre quand le kart quitte le tremplin : aucune roue au sol depuis 3 ticks, moins de 0,5 s après la dernière roue sur le tremplin. Elle se replie dès qu'une roue touche le sol, sous l'eau, ou avec le pélican ;
  - en vol, **Haut (l'accélérateur) pique** : plus vite (1,2 × la vitesse max du kart), plus raide (pente 0,17) ; **Bas (le frein) cabre** : plus lent (0,7 ×), plus à plat (pente 0,08) ; ni l'un ni l'autre : un vol plané (0,95 ×, pente 0,1). La descente est donc la même à toutes les vitesses (une pente, pas une vitesse de chute), 1,2 m/s au moins. **Gauche et Droite** font virer (1,1 rad/s à fond). La vitesse visée suit la vitesse max du kart, donc ses stats « air », ses croquettes et un boost en cours ; la maniabilité « air » règle le virage. Le saut du tremplin se fond dans la glisse (en 0,4 s environ) ; même un kart qui quitte le tremplin au ralenti s'envole ;
  - pas de gravité en vol : la glisse décide de la descente. Le kart reste droit (la caméra et les collisions ne changent pas) ; il ne s'incline qu'à l'écran : le nez plonge en piqué, se lève en cabré, le kart se penche dans les virages ;
  - **en vol, les planeurs ne se touchent pas entre eux** (les ailes font 4 m de large et le vol dure 2 à 3 s ; ils touchent tout le reste : la piste, les karts au sol, les objets), et un choc ne les fait jamais pivoter plus vite que leur propre virage à fond ;
  - **hors-piste** : un planeur au-dessus des murets n'est pas hors de la piste tant qu'il vole ; posé au-delà, il l'est (le pélican vient 1 s plus tard, comme toujours). Au-dessus du vide, il « tombe » sous le bas du vide moins 5 m (dans l'eau du lagon) ;
  - **les bots** visent leur ligne comme au sol (avec le virage du planeur) et piquent tant qu'ils sont à plus de 2 m au-dessus d'elle.
- **Sous l'eau** :
  - un **lac** (`lakes` : un rectangle creusé dans le sol jusqu'à son fond, plein d'eau jusqu'à son niveau, juste sous le sol) ; la route peut y plonger, et seulement là. Le kart est sous l'eau quand son centre est sous la surface de plus de 0,15 m ; il en sort quand il est au-dessus de plus de 0,15 m ;
  - sous l'eau : les stats « eau », la vitesse max × 0,9, la gravité × 0,5 (les sauts et les petits sauts flottent : leur durée maximale en l'air s'allonge d'autant, un petit saut finit toujours en drift), l'eau freine plus (décélérations × 1,6) et ralentit les sauts (la vitesse verticale s'éteint à 1,2 /s hors du sol) ;
  - à l'écran : des bulles montent derrière le kart, une gerbe de gouttes à l'entrée et à la sortie ; sous l'eau, la caméra voit tout en bleu (un voile sur l'écran, sous le HUD).
- **Anti-gravité** (#78 : un mur à 90° ; #94 : la route à l'envers, un plafond à 180° ; #96 : les zones en virage et les objets qui volent dans une zone) :
  - une **zone d'anti-gravité** est marquée dans le RON du circuit (`anti_gravity: [(from: 5.0, to: 15.0)]`, en paramètres de la ligne centrale, comme les vides) ; la route y est plate à ses deux bouts (pour y entrer et en sortir), et c'est le seul endroit où elle peut pencher **au-delà de 45°, jusqu'à 180°** (90° : un mur ; 180° : la route à l'envers, les karts roulent dessous, #94) : un circuit qui penche plus ailleurs est refusé. Dans une zone, le dévers suit une courbe lisse (sans à-coup aux points du tracé), pour que les karts loin du centre ne soient pas jetés en l'air quand la route se tord. **Une zone peut tourner** (#96) : relevée dans le virage (sa normale vers l'intérieur : la route ne fuit pas sous les karts), jusqu'à un mur autour du virage (90°) ; un virage relevé à 90° est une ligne droite dans le plan de la route : on le prend sans braquer ;
  - **à l'écran** : la route de la zone a ses bordures bleues et blanches, deux bandes bleues le long de la route et une bande bleue en travers à l'entrée et à la sortie ; ses plots de boost sont bleu cyan. **Les roues pivotent** à plat sous le kart (face extérieure en bas) en 0,25 s en entrant, et se redressent en sortant (visuel seulement). Le ruban d'une zone a son dos (40 cm derrière la route) ; les jupes ne descendent jusqu'au sol que sous une route qui regarde vers le haut (jusqu'au mur) : rien ne pend d'un plafond à travers les karts qui roulent dessous (#94) ;
  - **entrer, sortir** : une roue sur la route de la zone met le kart en anti-gravité, une roue sur une autre route l'en sort ; en l'air (petit saut, saut), il reste comme il était. Jamais sous le planeur ni sous l'eau ; le pélican l'en sort ;
  - **la gravité du kart** tire vers la route sous ses roues (sa normale), aussi fort que la gravité du monde ; en l'air, vers la route de la zone en dessous (cherchée jusqu'à 8 m), sinon vers la dernière touchée. Au sol, le kart s'aligne sur la normale de la route (12 /s : la route se tord, il suit) ; en l'air, il se redresse vers elle (comme ailleurs vers le haut du monde). **Au sol, la route guide le kart** (#96) : là où elle s'enroule le long de sa ligne centrale (un virage relevé), sa vitesse tourne avec elle, comme sur un rail : il n'est pas écrasé contre la route par la courbe, ses ressorts ne portent que sa gravité (`anti_gravity_guide`, activé). Une zone droite et à plat (le mur, le plafond) ne s'enroule pas, quelle que soit sa torsion : rien n'y change. **Un kart à plus d'1 m au-dessus de la route est tenu 3 fois plus fort** (sa gravité plus 2 fois elle) : là où la route se tord sous un kart en l'air, il ne passe jamais au-dessus d'un muret. Tout le reste (drift, mini-turbos, petits sauts, freinage, croquettes) se fait dans le repère de la route : la conduite est la même sur le mur et sous le plafond, avec les stats « anti-gravité » (§ 9). Arrêté sous un plafond, un kart y reste (comme dans MK8DX) ;
  - **tombé** (#94) : un kart en l'air qui ne trouve plus aucun sol le long de son haut (sorti d'un plafond par-dessus ses murets) se décroche : la gravité du monde revient, il tombe vers le sol. Sous un plafond, un kart à plus de 5 m sous la route (le même `fall_depth` qu'ailleurs, compté du plafond vers le sol) est tombé : le pélican vient aussitôt, et le dépose avant la zone (jamais dans une zone). Sous un plafond, le pélican sort d'abord le kart de sous la route (à côté du ruban, sous ses murets : `tow_clearance`, 1,5 m), puis le soulève : jamais à travers la route (#96) ;
  - **le spin boost** (MK8DX) : deux karts en anti-gravité qui se touchent à au moins 3 m/s (vitesse de rapprochement) reçoivent tous les deux un boost de +20 % pendant 0,6 s (un mini-turbo bleu). Les plots de boost de la zone donnent le boost habituel. **Pas de patte en anti-gravité** (§ 15, « Immunités ») : ni coup, ni attrape, dans un sens comme dans l'autre ; une laisse lâche ;
  - **la caméra** tourne avec le kart : son haut suit celui du kart par un ressort (`CameraTuning::up_frequency`, 6 rad/s), sans à-coup ; elle reste derrière et au-dessus du kart dans le repère de la route, et revient exactement au haut du monde à la sortie ;
  - **les objets** posés, qui roulent ou rebondissent sur la route (pelure, pelote, bombe) le font dans le repère de la route de la zone : une pelure tient sur le mur et sous le plafond, une pelote y roule. **Ceux qui volent aussi** (#96) : ils sortent du kart dans le repère de la route ; la bombe lancée devant part en cloche le long de la normale de la route et explose là où elle retombe dessus ; le boomerang-os file au ras de la route et tourne autour de sa normale ; l'oreiller géant vole à 4 m de la route le long de sa normale, plane au-dessus du leader le long de la normale de la route sous lui (sous un plafond : en dessous de lui, jamais dans la route) et tombe sur lui le long d'elle ; l'abeille suivait déjà la route le long de sa normale. **Les croquettes perdues** sautent le long de la normale et retombent sur la route, derrière le kart (sous un plafond aussi, au lieu de tomber au sol). À l'écran, un objet au-dessus d'une zone est tourné dans le repère de sa route (une pelure à plat sur le mur) ;
  - **les bots** suivent leur ligne sur le mur, sous le plafond et autour d'un virage relevé : dans une zone et autour d'elle, la ligne est tracée dans les coordonnées de la piste (#96 : la route déroulée sur un plan, sa courbure mesurée autour de sa normale) ; un virage relevé à 90° est une ligne droite pour eux (ni freinage ni drift, sans braquer). Ils voient les karts autour d'eux dans le repère de la route, et les visent de même (le boomerang-os tourne autour de la normale de la route, comme ils le visent) ;
  - **en ligne** : le dernier bit de l'état du kart (bit 7 : en anti-gravité) ; les autres voient son corps tourné sur le mur et ses roues pivotées, et l'étiquette de son pseudo le long de son haut (#96 : sous un plafond, en dessous de lui). Le spin boost : chaque pair donne le boost à son propre kart ;
  - **le Ruban Réglisse** (#78, 6e circuit ; #94 ; #96) : sur un remblai de 9 m, le long du côté nord, le ruban se tord jusqu'à devenir un mur (90°, 50 m), roule dessus, puis se remet à plat avant le virage, qui reste sur le remblai ; à l'est, un crochet : **un virage relevé** (#96 : le long de la ligne droite est, le ruban se tord vers la gauche jusqu'à un mur autour d'un long virage à gauche de 90°, 60 m de rayon, qu'on prend sans braquer, puis se remet à plat), puis une longue épingle à droite, où les bots driftent, qui monte sur le côté sud ; là, à 10 m du sol, **le ruban se retourne** (vers la gauche, par le mur, jusqu'à 180°) : les karts roulent dessous, à l'envers, sur 25 m au moins, puis il se remet à plat avant le virage qui redescend sur la ligne droite de départ.
- **En ligne** : trois bits de l'état du kart (planeur, sous l'eau, anti-gravité) ; les autres voient l'aile, l'inclinaison, les bulles, le kart sur le mur ou sous le plafond et ses roues pivotées.
- **Le Lagon Duvet** (#20, le circuit de démonstration) : départ en haut d'une falaise, envol au-dessus du lagon, atterrissage sur la plage, puis la route plonge sous l'eau du lagon avant de remonter la falaise.

## 11. Aides et accessibilité (MK8DX)

**Les aides à la conduite ne sont pas prévues pour l'instant** (décision de Max du 2026-10-13, #17). Pour mémoire, ce que fait MK8DX :

- **Direction assistée** : garde le kart sur la piste dans les virages difficiles, avec un témoin visible sur le kart. Elle désactive l'ultra mini-turbo.
- **Accélération automatique**.
- Les deux se règlent par joueur. Elles sont désactivées par défaut en ligne, mais restent autorisées.

Ce qui reste dans le code : la charge réduite du drift et le violet désactivé (§ 3), derrière un interrupteur par kart (`KartAssists`) que rien n'active en jeu.

Le confort qui existe déjà (#17) :

- **Options** (touche **O**, voir § 13) : un menu simple, mémorisé dans le navigateur, prévu pour accueillir d'autres options. Pour l'instant, la disposition du clavier (AZERTY ou QWERTY) et, depuis #80, les **noms au-dessus des karts** en ligne (oui par défaut).
- **Regarder derrière** (§ 13).

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
| Objet (maintenir = traîner ; avec Bas/S ou C = lancer derrière) | Shift ou E | LB ou LT (avec B ou X/Y = derrière) |
| Interaction (appui court = coup de patte, maintien = attraper, § 15) | X ou F | RB |
| Regarder derrière (tenir) | C | X / Y |
| Options | O | — |
| En ligne (salon) | L | — |
| Jouer solo : championnat, contre-la-montre, circuit seul, miroir, bots, objets (#99) | N | — |
| Menu en course (pause en solo, #56) ; sur l'accueil : une course tout de suite (#89) | Échap | Start |

- Les touches sont des **positions physiques** : W/A/S/D se lisent Z/Q/S/D sur un clavier AZERTY. X, F, E et C portent la même lettre en AZERTY et en QWERTY.
- **Disposition du clavier** (#17, menu Options) : **AZERTY** (par défaut, le jeu est en français) ou **QWERTY**. Les touches restent les mêmes sous les doigts dans les deux cas. Le choix change :
  - les **lettres affichées** (menu Options, écran Pilote) : Z/Q/S/D ou W/A/S/D ;
  - en AZERTY, le jeu lit **en plus** les positions Z (accélérer, haut) et Q (gauche). C'est ce que certains navigateurs envoient quand ils donnent les lettres au lieu des positions (Firefox qui résiste au pistage, bureaux à distance, machines virtuelles). Sur un vrai clavier AZERTY, ces positions sont les touches W et A, que le jeu n'utilise pas : aucun conflit. En QWERTY, Z (à côté de X) et Q ne font rien.
- **En planeur** (#20, § 10) : les mêmes touches. Haut (l'accélérateur) pique, Bas (le frein) cabre, Gauche et Droite virent ; à la manette, A pique, B cabre et le stick vire.
- **Regarder derrière** (#17) : la caméra passe devant le kart et regarde en arrière **tant que** C (ou X / Y à la manette) est maintenu, d'un coup, comme dans MK8DX ; au relâchement, elle revient exactement à la caméra de poursuite. Ça marche aussi sur la grille et après l'arrivée. Avec le bouton Objet, C lance aussi l'objet derrière (§ 8) : on voit où il part.
- **Options** (#17) : **O** ouvre et ferme le menu (même lettre et même place en AZERTY et en QWERTY, sans Fn sur Mac), en course comme depuis l'écran Pilote, par-dessus lequel il s'ouvre. Haut / Bas : la ligne ; Gauche / Droite (ou Entrée, Espace) : changer ; **Échap** ou O : fermer. Le jeu est en pause tant qu'il est ouvert (en ligne, non : voir plus bas ; sur l'accueil non plus, #89), et un changement s'applique tout de suite. Le menu liste aussi les commandes, avec les lettres du clavier choisi.
- **Interaction** : X se tient de la main gauche à côté de Shift et d'Espace quand on conduit aux flèches ; F est l'équivalent de E pour qui conduit avec W/A/S/D (Z/Q/S/D). À la manette, RB se tient à l'index pendant que le pouce garde A (accélérer) : RB quitte donc le saut/drift, qui reste sur RT.
- **Ni Ctrl ni Alt** : Ctrl + W (Cmd + W sur Mac) ferme l'onglet et le navigateur ne laisse pas la page l'empêcher ; Alt ouvre les menus de certains navigateurs.
- Touches de debug déjà prises : B (boost), R (pélican), F3 (overlay), I (un objet dans la main, #18 : même lettre et même place en AZERTY et en QWERTY, sans Fn sur Mac) ; Entrée et Espace relancent la course depuis le classement.
- **Jouer solo** (#19, #99) : **N** (« nouvelle course » : même lettre et même place en AZERTY et en QWERTY, sans Fn sur Mac) ouvre et ferme l'écran « Jouer solo », depuis la course ou l'écran Pilote (qu'il remplace) ; le bouton « Jouer solo » de l'accueil et du menu en course aussi. À gauche, ses lignes : le **mode** (Championnat, Contre-la-montre, Circuit seul) ; pour un championnat, la **formule** (une coupe ou personnalisé), puis la **coupe**, ou le **nombre de courses** et les **circuits** (au hasard ou au choix) ; sinon, le **circuit** ; puis le **miroir**, les **bots** et les **objets** (pas pour le contre-la-montre). À droite, les courses : chacune son numéro, sa **vignette** (le circuit vu de dessus, aux couleurs de son biome, en miroir s'il l'est) et son nom ; au choix, une ligne par course ; le circuit seul ou du contre-la-montre en grand, avec le record du contre-la-montre. Haut / Bas (ou Z/S en AZERTY, W/S en QWERTY) : la ligne (on en fait le tour) ; Gauche / Droite (Q/D ou A/D) : le choix ; **Entrée** lance la course, le championnat ou le contre-la-montre ; **Échap** ou N referment l'écran sans rien changer. À la manette : la croix ou le stick (un pas par poussée), A, B ou Start. À la souris : le survol donne le focus à une ligne ; un clic sur « < » ou « > », « C'est parti ! » ou « Retour ». Le jeu est en pause tant qu'il est ouvert (sauf sur l'accueil : son fond continue, #89). En ligne, N ne fait rien (l'hôte choisit dans le salon). Dans un championnat, Entrée (ou Espace, A à la manette) sur l'écran de classement passe au classement général, puis à la course suivante. Apparence : `docs/UI_CHARTE.md` § « L'écran Jouer solo (#99) ».
- **L'accueil** (#89) : on y arrive au lancement du jeu (avec `?room=…` dans l'adresse, directement dans le salon, sans lui), quand on quitte une course (« Quitter » dans le menu en course) et quand on quitte un salon. Le titre du jeu en grand et quatre boutons, dans cet ordre : **Jouer solo** (son écran, N, #99), **Multijoueur** (le salon, L : créer ou rejoindre une partie), **Options** (O), **Pilote** (l'écran du pilote et du kart, G, #98). Haut / Bas (ou Z/S en AZERTY, W/S en QWERTY) : choisir ; Entrée ou Espace : valider ; **Échap** : une course tout de suite sur le circuit du fond (« rouler tout de suite »). À la manette : la croix ou le stick, A, B ou Start. À la souris : le survol donne le focus, le clic valide. Un écran ouvert depuis l'accueil y revient quand on le ferme sans lancer de course. **Derrière, le jeu tourne** : quelques karts (5, des animaux, des pièces et des couleurs variés, celui du joueur parmi eux) font une course sans fin sur le circuit du moment et se donnent des coups de patte et s'attrapent bien plus souvent qu'en course (§ 15), groupés ; une caméra de bande-annonce change de plan toutes les 6 s, sans jamais couper. Ni HUD ni son de course derrière ; l'accueil ne met jamais en pause, ni les écrans qu'il ouvre. Réglages : `HomeTuning` (panneau d'admin). Apparence : **la charte graphique, `docs/UI_CHARTE.md`** (palette, police, boutons, animations, navigation, ton des textes), validée par Max le 2026-10-17.
- **Menu en course** (#56) : **Échap** (ou Start à la manette) quand aucun autre écran n'est ouvert : Échap ferme d'abord celui-là. **Reprendre** en premier, les quatre boutons de l'accueil, puis **Quitter** (retour à l'accueil ; en ligne, on quitte aussi la partie). En solo, le jeu est en pause tant qu'il est ouvert ; **en ligne, jamais** (le kart roule sur son élan, seules les commandes sont inactives) ; en ligne, « Jouer solo » est grisé (l'hôte choisit). Son écran de pause viendra avec #100.
- **Pilote** (#15, #98) : l'écran du pilote et de son kart s'ouvre depuis l'accueil (« Pilote », #89) et le menu en course, au lancement avec `?room=…` (le salon passe après lui : un écran à la fois) et avec **G** (« garage » : même lettre et même place en AZERTY et en QWERTY, sans Fn sur Mac). On y choisit, de haut en bas : son **pseudo** (un champ de texte), son **personnage** (une grille de tous les animaux, chacun en portrait de 3/4 face avec son nom, une ligne par gabarit), son **kart** (la carrosserie), ses **roues**, son **aile** et sa **couleur** (les 12 pastilles de la palette). À gauche, **le kart vu de 3/4 face**, en 3D, avec le personnage dedans qui se dandine, rebondit et s'écrase en boucle ; sur la ligne de l'aile, elle s'ouvre. Dessous, les barres de stats (§ 9). Flèches haut et bas (ou Z/S en AZERTY, W/S en QWERTY) : la ligne (on fait le tour : la ligne du pseudo est au-dessus de celle du personnage) ; gauche et droite (Q/D ou A/D) : le choix ; **Entrée** garde le choix (sur la ligne du pseudo : on écrit), **Échap** ou G referment l'écran sans rien changer. À la manette : la croix ou le stick (un pas par poussée), A, B ou Start. À la souris : le survol donne le focus à une ligne ; un clic sur « < » ou « > », un portrait, une pastille, « Valider » ou « Retour ». **Le choix gardé** : depuis le jeu en solo, la course repart avec lui ; depuis l'accueil, retour à l'accueil (son fond roule toujours) ; en ligne, il part tout de suite chez les autres et sert à la course suivante. **Le pseudo** : 12 caractères, ceux que la police du HUD sait écrire (lettres accentuées comprises, sans emoji) ; vide, c'est le nom de l'animal (écrit en gris dans le champ). Il se tape après Entrée sur sa ligne ou un clic dedans ; tant qu'on tape, aucune touche ne pilote le kart, ne change de ligne ni ne déclenche de raccourci ; Entrée, Échap ou un clic ailleurs rendent les touches. Chaque changement est le pseudo tout de suite : mémorisé dans le navigateur (`?pseudo=` dans l'adresse reste prioritaire), envoyé aux autres en ligne. Le jeu est en pause tant que l'écran est ouvert depuis le jeu en solo ; en ligne et sur l'accueil, jamais. Apparence et navigation : la charte, `docs/UI_CHARTE.md` § « L'écran Pilote (#98) ».
- **En ligne** (#7) : **L** (« en ligne » : même lettre et même place en AZERTY et en QWERTY, sans Fn sur Mac) ouvre et ferme le salon : « Créer une partie », le lien d'invitation, la liste des joueurs (nom, animal, ping, hôte), « Prêt » et « Quitter la partie ». Il s'ouvre tout seul avec `?room=CODE` dans l'adresse (`--room CODE` en natif). Il se manie à la souris et **ne met pas le jeu en pause**. Tant que le menu d'options est ouvert, L ne fait rien. L'hôte y choisit le circuit et lance la course quand tout le monde est prêt (#9) ; depuis #99, aussi le **miroir**, les **bots** (oui : ils complètent la grille à 12 ; non : seuls les joueurs courent) et les **objets** (non : aucune boîte sur la piste), pour le roulage libre comme pour la course ; les autres joueurs voient ses choix dans le salon (« Réglages de l'hôte : … · Miroir : non · Bots : oui · Objets : oui »). Depuis #64 : le salon s'ouvre aussi tout seul en arrivant dans la partie et à la fin de chaque course, et se ferme au départ ; l'action principale est en gros (« Lancer la course » chez l'hôte, grisé avec sa raison, « Bob n'est pas prêt. » ; « Prêt ! » chez les autres ; l'hôte n'a pas à se dire prêt) ; hors course, **un bandeau en haut de l'écran** dit le salon, ses joueurs et quoi faire (« Salon ABCDE · 3 joueurs · Tu es l'hôte : choisis le circuit puis lance la course (L) », « … · En attente de l'hôte (Ana)… »). Depuis #80, **le pseudo** (#98 : il se tape dans l'écran Pilote, plus haut ; le salon l'écrit en haut, « Ton pseudo », avec « Change-le dans Pilote (G). ») arrive tout de suite chez les autres et s'affiche dans le salon, au classement et au-dessus du kart (sans ses accents dans le HUD : « Éloïse » → « Eloise »). Tant que l'écran Pilote est ouvert, le salon et son bandeau s'effacent : ils reviennent quand il se ferme.
- **Pas de pause en ligne** (#9, décision de Max) : dans une partie en ligne, aucun menu ne met le jeu en pause (options, Pilote, panneau d'admin, salon) : les autres continuent de courir. Le kart du joueur roule en roue libre tant qu'un menu a les touches. Un nouveau kart choisi pendant une course sert à la course suivante.
- **Panneau d'admin** (#36) : **P** l'ouvre et le ferme (même lettre en AZERTY et en QWERTY, sans Fn sur Mac, contrairement à F1 à F12), tout comme le petit bouton « admin » en bas de l'écran ; `?admin` dans l'adresse de la page l'ouvre au démarrage (`--admin` en natif). Quand on écrit dans un de ses champs de texte, le jeu ne reçoit plus aucune touche.

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

**Les écrans** (menus, salon, choix du kart, pause, résultats) suivent la charte graphique de #56 : **`docs/UI_CHARTE.md`**. Des formes rondes et des couleurs de bonbons, la police Fredoka (avec les accents), des boutons qui rebondissent et s'écrasent au clic, la même navigation au clavier, à la souris et à la manette, des textes courts avec un brin d'humour.

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
| Invincible (après un objet, super câlin, pie voleuse…) | oui | oui | non | non |
| Pélican dépanneur | non | non | non | non |
| Anti-gravité (§ 10, #78 : le spin boost à la place) | non | non | non | non |
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

### Notre implémentation (#34)

Toutes les valeurs sont dans la Resource `InteractionTuning` (panneau d'admin) ; celles des bots dans `BotSkill`.

- **Bouton** : X ou F, RB à la manette. Tenu moins de 12 ticks (0,2 s), c'est un coup au relâchement ; à partir du 12ᵉ tick, une attrape.
- **Coup de patte** : la poussée est une vitesse à part (`Shove`), ajoutée par-dessus la conduite et qui s'éteint seule ; le grip et le drift n'y touchent pas, un muret ou un kart qui l'arrête y met fin. Mesuré, une seconde après : 1,34 m d'écart à poids égal, 1,74 m du plus lourd sur le plus léger, 0,94 m dans l'autre sens ; vitesse × 0,90 ; 1 croquette (même protection d'1 s que les chocs de karts).
- **Coup ou griffe dans le vide** : le kart glisse de 0,3 m du côté du coup et le personnage mouline des bras 0,6 s.
- **Lien** : un ressort amorti qui ne fait que tirer (30 m/s² par mètre étiré, 10 /s sur la vitesse d'éloignement). L'attrapeur rejoint la vitesse de la cible : entre sa propre vitesse max et celle de la cible, seul le lien le tire.
- **Frein de la cible** (ajout, à valider par Max) : 25 % de la force du lien ne suffisent pas à l'objectif de réglage. Le moteur, très fort près de la vitesse max, et les boosts (40 m/s²) rattrapent aussitôt quelques m/s² de freinage : mesuré, 0,4 % de vitesse perdue pour une cible en boost. Tant qu'elle est tenue, la vitesse max de la cible baisse donc aussi de 10 %, multiplié par son facteur de Poids (`tether_target_brake`). Mesuré sur une prise complète : 10,0 % à poids égal, 12,5 % pour une cible légère (Poids 0), 8,3 % pour une lourde (Poids 20). La cible récupère sa vitesse dès qu'elle est libre.
- **Jet** : mesuré 2,0 m d'écart, contre 1,34 m pour un coup.
- **Bots** : 10, 30 et 60 % des occasions. Les Difficile frappent un kart placé du côté du bord le plus proche avec une chance × 1,33, les autres × 0,67. Ils attrapent un kart qui va au moins 2 m/s plus vite qu'eux : jamais pour les Facile, 30 % des occasions pour les Normal, 70 % pour les Difficile. Ils tiennent 1,2 s (Normal) ou 1,6 s (Difficile), puis lâchent ; les Difficile jettent la cible de son côté, loin d'eux. Tenus, ils sautent après 1 s, 0,5 s ou 0,25 s selon leur niveau.
- **Ressenti** :
  - fait : le bras (capsule) et la patte qui se lève dès l'appui, frappe, s'étire pendant l'attrape (l'élastique) et revient en « boing » ; le « pof » (losange jaune) ; le personnage qui se penche en moulinant ou se retourne vers son attrapeur ; un léger tremblement d'écran pour le joueur frappé ; la jauge de recharge dans le HUD ;
  - le mini-ragdoll (#12) : le pilote frappé ou touché par un objet ballotte environ 1 s (la tête jetée vers la patte, puis des ressorts plus mous), visuel seulement ;
  - pas encore fait : le cri, la vibration de la manette et les sons.
- **Sans interactions** : `InteractionTuning::enabled` les coupe. Les tests de conduite des bots tournent ainsi.

## 16. Priorités

| Phase | Contenu |
|---|---|
| **1 – Conduite (solo)** | Conduite de base, caméra, circuit, drift et 3 paliers, départ fusée, figures, aspiration, panneaux, croquettes, dépanneur, course 3 tours avec classement, bots, statistiques et gabarits |
| **1b – Objets (solo)** | Boîtes, roulette, 2 emplacements, traîner, distribution par distance, objets de phase 1, réactions quand on est touché et invincibilité |
| **1c – Interactions (solo)** | Coup de patte et attrape entre karts (§ 15), effet du Poids, immunités, bots. Avant le réseau, pour que la synchronisation les prévoie dès le départ |
| **2 – En ligne** | Rooms P2P, synchronisation des karts, course synchronisée avec objets et croquettes, signalisation de prod. Plus les objets de phase 2. |
| **3 – Contenu** | Vrais modèles, personnages mous, Grand Prix et miroir, anti-gravité, planeur, eau, objets de phase 3 |
