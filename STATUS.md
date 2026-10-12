# État du projet

_Mis à jour par Claude à chaque PR._

## Ce qui marche
- **Circuit de test « Prairie Pompon »** (#3), généré depuis `assets/tracks/prairie.track.ron` :
  - spline Catmull-Rom fermée, avec largeur, dévers et bas-côté par point de contrôle ;
  - le tracé : longue ligne droite de départ, épingle (12 m de rayon au sommet), ligne droite arrière avec un tremplin, chicane, virage relevé surélevé ; 536 m ;
  - un seul mesh pour route, vibreurs rouges et blancs, bas-côtés en herbe, murets et jupes (couleurs par sommet, un seul draw call) ;
  - colliders trimesh Rapier par surface (route, herbe, murets) ;
  - 21 checkpoints automatiques, ligne de départ et 12 places de grille en quinconce ;
  - dans les données mais pas encore actifs (repères visuels seulement) : zone de sable, panneaux de boost, boîtes d'objets simples et doubles, croquettes, points de dépannage (un par checkpoint par défaut) ;
  - décor : herbe tout autour, une soixantaine d'arbres en primitives ;
  - test de bout en bout : un pilote automatique simple boucle un tour en 19,9 s en passant tous les checkpoints dans l'ordre, sans sortir des murets ni décoller ailleurs qu'au tremplin ;
  - un fichier de circuit absent ou invalide (champ mal orthographié, valeurs absurdes) est signalé dans la console, et le jeu se rabat sur le terrain de test.
- Le terrain plat de #1 reste disponible pour les tests (`TrackChoice::TestGround`).
- **Kart arcade jouable** (#1) sur un terrain de test : route, zone d'herbe à gauche, caisses et mur à droite, rampe droit devant.
  - Corps Rapier dynamique posé sur 4 raycasts de suspension (ressort + amortisseur), gravité ×2,2, redressement automatique en l'air.
  - Courbe moteur (0 → 28,5 m/s en 2,5 s), frein puis marche arrière sur le même bouton, grip latéral fort, direction pilotée en lacet.
  - Hors-piste à 55 % de la vitesse max (ignoré pendant un boost), composant `Boost` générique.
  - Machine à états `Grounded` / `Airborne` (les états drift, figure, touché viendront avec leurs issues).
  - Simulation en `FixedUpdate` 60 Hz, rendu interpolé entre deux ticks (`src/interpolation.rs`).
  - Entrées clavier + manette → `KartInput` ; touches de debug : **B** = boost, **R** = retour au départ.
  - Tests de scénario headless (`src/kart/scenario_tests.rs`) : accélération, herbe, boost, frein/marche arrière, virage 3 s, saut de rampe, choc frontal contre un mur.
  - Test de bout en bout (`src/lib.rs`) : flèche haut maintenue sur le vrai terrain de test → le kart saute la rampe et retombe sur ses roues.
  - Vérifié dans Chromium headless (WebGL2 logiciel) : le jeu s'affiche, le kart roule et tourne au clavier. Le ressenti et les fps réels restent à juger par Max.
- **Caméra de poursuite** (#2, `src/camera/`) :
  - derrière le kart, elle suit sa pose *interpolée* ; ressorts critiques amortis (aucun dépassement, même rendu à 30, 60 ou 144 fps) ;
  - elle pivote en virage avec un peu de retard, regarde devant le kart et anticipe les virages selon la vitesse de lacet ;
  - FOV de 60° à l'arrêt, 70° à pleine vitesse, +6° en boost ;
  - un rayon kart → caméra l'empêche de passer derrière un mur ou sous le sol ;
  - hook `CameraLean` (−1 à 1) prêt pour le décalage latéral du drift (#4) ;
  - réglages dans la Resource `CameraTuning`.
- CI : fmt, clippy, tests et check wasm sur `main`, `dev`, les PR et les branches `claude/**`. Déploiement GitHub Pages depuis `dev`.
- Fusion automatique des branches `claude/**` dans `dev` quand la CI passe (`automerge.yml`).

## Décisions
- 2026-10-12 : Bevy 0.20, physique bevy_rapier3d 0.37. Avian, lightyear et bevy_ggrs ne supportent pas encore 0.20.
- 2026-10-12 : réseau P2P en maillage complet via matchbox_socket. Chaque joueur fait autorité sur son kart, l'hôte arbitre la course. Pas de serveur de jeu (voir `docs/ARCHITECTURE.md`).
- 2026-10-12 : WebGL2 par défaut pour la compatibilité, WebGPU à évaluer plus tard.
- 2026-10-12 : formes primitives d'abord, packs d'assets CC0 ensuite (issue dédiée).
- 2026-10-12 : `dev` = branche par défaut, alimentée automatiquement par Claude (fusion auto si CI verte) et déployée sur Pages. `main` = version stable, fusionnée à la main par Max.
- 2026-10-12 : référence de gameplay = Mario Kart 8 Deluxe (mécaniques et valeurs dans `docs/GAME_DESIGN.md`), avec un contenu 100 % original.
- 2026-10-12 (#1) : `bevy_rapier3d` sans ses features par défaut (seulement `dim3`) : pas de rendu debug ni de chargement de colliders depuis des meshes, pour un wasm plus léger. À rajouter si le circuit (#3) en a besoin.
- 2026-10-12 (#1) : la vitesse du kart est réécrite à chaque tick depuis nos courbes (`src/kart/physics.rs`, logique pure testée) ; Rapier ne gère que les collisions, la gravité et la suspension. Tous les réglages sont dans la Resource `KartTuning` (`src/kart/tuning.rs`).
- 2026-10-12 (#1) : courbe moteur `v(t) = vmax · (1 − (1 − t/T)²)` : départ nerveux, vmax atteinte en `T` = 2,5 s, 95 % dès 1,9 s.
- 2026-10-12 (#1) : le hors-piste est mélangé roue par roue (2 roues dans l'herbe = moitié du ralentissement), pour éviter les à-coups en bordure.
- 2026-10-12 (#1) : la simulation (`SimulationPlugin`) n'a ni fenêtre ni rendu ni périphérique d'entrée ; `GamePlugin` y ajoute le terrain, les entrées joueur, les visuels et la caméra. Les touches sont des positions physiques : W/A/S/D = Z/Q/S/D sur AZERTY.
- 2026-10-12 (#1) : les murs et caisses ont une restitution de 0,3 (`track::WALL_RESTITUTION`), le kart 0 avec la règle `Max` : on rebondit sur les murs, pas sur le sol. Les surfaces de plus de 60° ne comptent pas comme sol (`min_ground_normal_y`), pour qu'un kart couché contre un mur se redresse au lieu de rouler dessus. À revoir pour l'anti-gravité (#20).
- 2026-10-12 (#2) : la caméra suit le kart exactement à l'horizontale et ne lisse que la *direction* (pivot en virage) et la hauteur (sauts). Un ressort sur la position entière ferait traîner la caméra de plusieurs mètres à pleine vitesse.
- 2026-10-12 (#25, retour de Max après le premier essai : « braquage un peu sec, un peu plus de vitesse, les sauts sont cools ») :
  - vitesse max 26 → **28,5 m/s** (≈ 103 km/h ; le game design part de 26, valeur « à régler ») ;
  - braquage −12 % : `steer_rate` 2,6 → **2,3** rad/s, `steer_rate_at_max_speed` 1,9 → **1,65** rad/s (rayon à fond ≈ 17,5 m au lieu de 13,7) ;
  - réponse du volant plus douce : `steer_response` 14 → **8** /s (63 % du braquage en 125 ms au lieu de 70 ms) ;
  - sauts inchangés (gravité, suspension, tremplins) ; un peu plus longs avec la vitesse.
- 2026-10-12 (#3) : format des circuits (`src/track/data.rs`) : les positions le long du tracé sont des **paramètres de spline** (`9.5` = à mi-chemin entre les points 9 et 10), plus faciles à écrire à la main que des mètres ; latéral de −1 (bord gauche) à 1 (bord droit). Les dépendances `ron` 0.12 et `serde` 1 étaient déjà compilées pour Bevy : aucun coût.
- 2026-10-12 (#3) : le circuit est chargé comme un asset Bevy (`*.track.ron`) : un nouveau circuit = un nouveau fichier. `AssetMetaCheck::Never` dans `main.rs`, sinon le web demande des `.meta` inexistants.
- 2026-10-12 (#3) : l'herbe autour du circuit est à −25 cm : la spline peut plonger de quelques centimètres sous ses points de contrôle avant une montée.
- 2026-10-12 (#3) : l'épingle fait 12 m de rayon au sommet, plus serré que le kart sans drift à pleine vitesse (≈ 13,7 m) : il faut lever le pied, ou drifter quand #4 arrivera. Le virage relevé est assez long pour un ultra mini-turbo.
- 2026-10-12 (#3) : correction d'un bug de #1 trouvé sur la grille (légèrement en pente) : à l'arrêt sur une faible pente, le kart reculait d'un millimètre par seconde et l'accélérateur ne faisait que freiner ce recul, indéfiniment. Les tests de #1 ne le voyaient pas (sol parfaitement plat). Test de régression ajouté.
- 2026-10-12 (#2) : la fonction « regarder derrière » n'est pas faite ici ; elle est prévue dans #17.
- 2026-10-12 (#1) : sous 30 m de hauteur, le kart revient au départ (filet de sécurité du terrain de test, en attendant le pélican dépanneur de #5).

## Prochaines étapes
- Milestone 1 « Ça roule » (solo) :
  - ~~kart~~ (#1), ~~caméra de poursuite~~ (#2), ~~circuit~~ (#3), drift à 3 paliers (#4), course (#5), bots (#6) ;
  - puis départ fusée, figures et aspiration ; croquettes ; stats et gabarits ; objets 1/2 ; aides de conduite.
- Milestone 2 « À plusieurs » : réseau 1/3 à 3/3, signalisation (attend un compte Cloudflare), objets 2/2.
- Milestone 3 « Contenu » : vrais modèles (attend les assets), personnages mous, Grand Prix et classes, anti-gravité / planeur / eau, objets de phase 3 et Bataille.

Le gameplay de référence est décrit dans `docs/GAME_DESIGN.md` (inspiré de Mario Kart 8 Deluxe). Les textes des issues sont dans `.github/backlog/`.

## Questions pour Max
- **Taille du wasm : 56 Mo** en release (déploiement de `dev` après #2, avec Rapier). C'est long à charger sur une connexion moyenne. Pistes, qui touchent la configuration de build et que je n'ai donc pas appliquées sans ton accord :
  - `strip = true` et `opt-level = "z"` dans `[profile.release]` ;
  - désactiver les features Bevy inutiles (audio, gltf… tant qu'on n'en a pas besoin) ;
  - vérifier que `wasm-opt` passe bien dans `trunk build --release` (`data-wasm-opt="s"`).

  Je peux en faire une issue si tu veux.

## Métriques
- Taille wasm (release, CI) : **56 Mo** au 2026-10-12, après #2 (kart + Rapier + caméra). Première mesure.
