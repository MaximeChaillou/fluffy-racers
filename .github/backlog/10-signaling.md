Signalisation de production, pour jouer en ligne sans serveur de jeu.

WebRTC a besoin d'un petit service de mise en relation. On le met sur **Cloudflare Workers + Durable Objects** (offre gratuite). Aucune donnée de jeu n'y passe.

## Ce que Max doit faire
1. Créer un compte Cloudflare (gratuit).
2. Créer un token API avec les droits « Workers Scripts: Edit ».
3. L'ajouter dans les secrets du repo : `CLOUDFLARE_API_TOKEN` et `CLOUDFLARE_ACCOUNT_ID`.

## À faire (Claude)
- Dossier `signaling/` : un Worker compatible avec le protocole de signalisation matchbox (une room = un Durable Object).
- Une GitHub Action qui déploie le Worker avec `wrangler` quand `signaling/` change sur `main`.
- Le jeu web utilise cette URL en prod et `localhost:3536` en dev.
- Ajouter des serveurs STUN publics à la config WebRTC.

## Critères d'acceptation
- Deux navigateurs sur deux réseaux différents (wifi + 4G) se rejoignent via le lien d'invitation.
