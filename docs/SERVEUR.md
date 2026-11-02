# Le serveur dédié

Le serveur dédié héberge les parties à la place du navigateur d'un joueur (#225). Il tourne sur une
machine à part, sans écran : un petit serveur loué (VPS), un vieux PC ou un Raspberry Pi 4 ou 5.
On l'installe une fois, et on n'y touche plus : il démarre avec la machine et redémarre tout seul
s'il plante.

- **Ce qu'il fait** : il est l'hôte de la partie, sans kart ni place. Les 12 places restent aux
  joueurs. Il relaie les états des karts, conduit les bots, arbitre les objets, les croquettes et
  la course, donne l'horloge et mène le championnat. Il n'a ni fenêtre, ni rendu, ni son, ni
  carte graphique : la course tourne à 60 Hz, comme chez un joueur, sans jamais attendre un rendu.
- **Comment on joue** : dans le jeu, « Créer une partie » va sur le serveur. Le joueur qui crée la
  partie en est le **chef** : il choisit la course ou le championnat, puis lance le départ. S'il
  part, le joueur suivant devient chef. Si une partie est déjà en cours, le jeu le dit (le mode, le
  circuit, les joueurs, où en est la course) et propose **Rejoindre** ou **Retour**. Quand tout le
  monde est parti, le serveur oublie la partie et attend la suivante.
- **Sans serveur**, rien ne change : le navigateur de celui qui crée la partie l'héberge.

## Ce qu'il faut

- Une machine Linux allumée en permanence, avec 1 Go de mémoire libre et un cœur de processeur
  (voir les mesures dans `STATUS.md`, « Métriques »).
- Docker (le plus simple), ou Rust pour compiler soi-même (« Sans Docker », plus bas).
- Deux ports ouverts vers cette machine :
  - **TCP 3536** : la mise en relation (un WebSocket), que le serveur contient ;
  - **UDP** : les connexions WebRTC des joueurs. Le serveur prend un port au hasard pour chaque
    joueur, entre **32768 et 60999** sous Linux.

## 1. Installer Docker

Sur la machine (Debian, Ubuntu, Raspberry Pi OS) :

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # puis déconnecte-toi et reconnecte-toi
```

## 2. Récupérer le jeu et lancer le serveur

```bash
git clone https://github.com/MaximeChaillou/fluffy-racers.git
cd fluffy-racers
docker compose up -d --build
```

La première fois, la compilation prend du temps : une dizaine de minutes sur un VPS, plus d'une
heure sur un Raspberry Pi. Ensuite, le serveur tourne tout seul : `restart: always` le relance
après un plantage et au démarrage de la machine.

Pour vérifier :

```bash
docker compose logs -f serveur   # « server: in room SERVEUR … »
```

Pour le mettre à jour (une nouvelle version du jeu change souvent le protocole : le serveur et la
page doivent avoir la même version) :

```bash
git pull && docker compose up -d --build
```

## 3. Ouvrir les ports

### Un VPS (une adresse publique)

Si le pare-feu `ufw` est actif :

```bash
sudo ufw allow 3536/tcp
sudo ufw allow 32768:60999/udp
```

Chez certains hébergeurs, il y a aussi un pare-feu dans leur interface web : ouvre les mêmes ports.

### Un PC ou un Raspberry Pi à la maison

- **Pour jouer sur le réseau de la maison**, rien à ouvrir.
- **Pour des joueurs de l'extérieur** :
  1. dans la box, redirige le port **TCP 3536** vers la machine du serveur ;
  2. donne au serveur un serveur STUN, pour qu'il connaisse l'adresse publique de la box. Dans
     `docker-compose.yml`, remplace la ligne `command` par :

     ```yaml
     command: ["--port", "3536", "--stun", "stun:stun.l.google.com:19302"]
     ```

  La plupart des box laissent alors passer l'UDP. Avec une box trop fermée, il faudra un relais
  TURN (#10).

## 4. HTTPS : quand la page du jeu est en HTTPS

La page de GitHub Pages est en `https://`. Un navigateur y refuse les adresses `ws://` (sauf
`localhost`) : il faut une adresse **`wss://`**, donc un nom de domaine et un certificat.

1. Un nom de domaine qui pointe vers la machine (gratuit avec [DuckDNS](https://www.duckdns.org),
   par exemple `fluffy.duckdns.org`).
2. Ouvre aussi les ports **TCP 80 et 443**.
3. Lance le serveur avec Caddy, qui obtient le certificat tout seul :

   ```bash
   DOMAINE=fluffy.duckdns.org docker compose --profile https up -d --build
   ```

L'adresse du serveur est alors `wss://fluffy.duckdns.org`.

Sans domaine, sur le réseau de la maison : ouvre le jeu depuis une page en `http://` (servie par
`trunk serve` sur ce réseau, voir le README, « Jouer à plusieurs »), et l'adresse `ws://` marche.

## 5. Donner l'adresse au jeu

L'adresse du serveur, c'est celle de sa mise en relation :

- `ws://192.168.1.30:3536` sur le réseau de la maison (l'adresse de la machine) ;
- `ws://203.0.113.7:3536` pour un VPS, depuis une page en `http://` ;
- `wss://fluffy.duckdns.org` avec HTTPS (étape 4).

Ouvre le jeu **une fois** avec `?server=` suivi de cette adresse, par exemple :

```
https://maximechaillou.github.io/fluffy-racers/?server=wss://fluffy.duckdns.org
```

Le navigateur la garde. Ensuite, « Multijoueur » puis « Créer une partie » va sur ton serveur. Le
lien d'invitation de la partie porte l'adresse du serveur : tes amis n'ont rien à régler.

Pour revenir au jeu sans serveur (le navigateur héberge) : ouvre le jeu une fois avec `?server=`
(vide).

## Sans Docker

Avec Rust installé (voir `rust-toolchain.toml`) et les bibliothèques de compilation :

```bash
sudo apt install pkg-config libasound2-dev libudev-dev
cargo build --release --bin fluffy-racers-server
sudo useradd --system fluffy
sudo mkdir -p /opt/fluffy-racers
sudo cp target/release/fluffy-racers-server /opt/fluffy-racers/
sudo cp -r assets /opt/fluffy-racers/
sudo cp deploy/fluffy-racers-server.service /etc/systemd/system/
sudo systemctl enable --now fluffy-racers-server
journalctl -u fluffy-racers-server -f
```

Le programme n'ouvre ni fenêtre ni son, mais il est lié aux bibliothèques `libasound2` et `libudev1`
(présentes sur presque tous les Linux).

## Les options du serveur

```
fluffy-racers-server --help
  --port 3536          le port de la mise en relation (WebSocket), à ouvrir en TCP
  --room SERVEUR       la partie (celle que le jeu connaît par défaut)
  --stun stun:…        un serveur STUN (un serveur derrière une box), aucun par défaut
  --signal ws://…      une mise en relation à part (matchbox_server), sans la sienne
```

## Dépannage

- **« Impossible de joindre le serveur de mise en relation »** : le port TCP 3536 (ou 443 avec
  HTTPS) n'est pas ouvert, ou l'adresse est fausse. Depuis une page en `https://`, l'adresse doit
  commencer par `wss://`.
- **Le salon reste sur « Connexion à l'hôte… »** : la connexion WebRTC ne passe pas. Vérifie les
  ports UDP (étape 3), puis le serveur STUN pour une machine derrière une box.
- **« Un joueur a une autre version du jeu »** : le serveur et la page n'ont pas la même version.
  Mets le serveur à jour (`git pull && docker compose up -d --build`).
