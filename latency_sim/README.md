# Simulateur de latence (pipeline 3 partitions)

Application egui pour simuler la latence d’un pipeline de traitement (débit 16 Mbps, paquets 64–1500 octets) avec 3 partitions, files d’attente G/G/1 et contrainte de budget (ex. 1 ms).

**[Capture d’écran de l’application](screenshot.png)**

---

## Lancer l’app

```bash
cargo run
```

---

## Algorithme

### 1. Modèle du pipeline

- **3 partitions en série** : P1 → (file) → P2 → (file) → P3.
- Chaque partition a un **temps de service** par paquet (en µs) :
  - **Coût fixe** \( a \) (µs/paquet) + **coût dynamique** \( b \times \text{taille} \) avec \( b \) en ns/octet.
  - Formule : \( P_k = a_k + \frac{b_k \times \text{taille}}{1000} \) (µs).
- Le **débit** (Mbps) et la **taille de paquet** (octets) donnent le **débit en paquets/s** :
  \[
  \lambda = \frac{\text{throughput\_Mbps} \times 10^6}{\text{taille\_octets} \times 8}.
  \]
- **Inter-arrivée moyenne** : \( T = 1/\lambda \) (en µs si \( \lambda \) en paquets/µs, ou \( T = 10^6/\lambda \) si \( \lambda \) en paquets/s).

### 2. Stabilité (goulot)

- Le pipeline peut suivre le débit **si et seulement si** aucun étage ne reçoit plus de paquets qu’il ne peut en traiter.
- **Goulot** : \( B = \max(P_1, P_2, P_3) \) (µs).
- **Condition de stabilité** : \( T \geq B \).  
  Si \( T < B \), la file devant l’étage le plus lent croît indéfiniment (régime instable).

### 3. Files G/G/1 (approximation de Kingman)

- Chaque étage est modélisé comme une **file G/G/1** :
  - **Utilisation** : \( \rho_k = P_k / T \) (pour l’étage \( k \)).
  - **Variabilité** : \( c_a^2 \) (carré du coefficient de variation des inter-arrivées), \( c_s^2 \) (carré du C.V. du temps de service).  
  Ex. Poisson → \( c_a^2 = 1 \), service déterministe → \( c_s^2 = 0 \).

- **Temps de séjour** (attente + service) à l’étage \( k \) (Kingman) :
  \[
  E[T_{\text{soj},k}] = P_k \left(1 + \frac{\rho_k}{1-\rho_k} \cdot \frac{c_a^2 + c_s^2}{2}\right).
  \]
  Si \( \rho_k \geq 1 \), on considère une latence infinie (instable).

- **Latence totale (avec files)** : somme des trois temps de séjour.

- **Taille moyenne de la file** (en paquets) devant l’étage \( k \) (hors le paquet en service) :
  \[
  L_{q,k} = \frac{\rho_k^2\,(c_a^2 + c_s^2)}{2(1-\rho_k)}.
  \]
  L’interface affiche la file entre P1→P2 et celle entre P2→P3.

### 4. Contrainte de budget

- **Budget** : ex. 1000 µs (1 ms).
- On compare la **latence totale avec files** à ce budget : si elle dépasse le budget (ou si le régime est instable), l’état est « NE RESPECTE PAS LA CONTRAINTE ».

### 5. Résumé des formules (une taille de paquet donnée)

| Grandeur | Formule |
|----------|--------|
| Débit paquets/s | \( \lambda = \frac{\text{Mbps} \times 10^6}{\text{taille\_octets} \times 8} \) |
| Inter-arrivée | \( T = 10^6/\lambda \) (µs) |
| Temps service étage \( k \) | \( P_k = a_k + b_k \times \text{taille}/1000 \) (µs) |
| Goulot | \( B = \max(P_1,P_2,P_3) \) |
| Stable | \( T \geq B \) |
| Sojourn étage \( k \) | \( P_k\bigl(1 + \frac{\rho_k}{1-\rho_k}\frac{c_a^2+c_s^2}{2}\bigr) \), \( \rho_k = P_k/T \) |
| File moyenne (paquets) | \( L_{q,k} = \frac{\rho_k^2(c_a^2+c_s^2)}{2(1-\rho_k)} \) |

---

## Capture d’écran

Le fichier [screenshot.png](screenshot.png) montre l’interface (barre de statut, panneau de paramètres, schéma du pipeline avec files, graphe latence vs taille de paquet). Pour le mettre à jour : lancer l’app puis capturer la fenêtre (macOS : Cmd+Shift+4 puis Espace).
