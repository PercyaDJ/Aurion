// ─── Aurion : fonctions communes à toutes les pages ────────

// fetch() qui lève une erreur lisible si le serveur répond une erreur.
async function apiFetch(url, options = {}) {
    const res = await fetch(url, options);
    if (!res.ok) {
        let msg = '';
        try { msg = await res.text(); } catch (_) { /* ignore */ }
        throw new Error(msg || ('Erreur ' + res.status));
    }
    return res;
}

// Échappe un texte avant insertion dans du HTML.
function aurionEscape(text) {
    const div = document.createElement('div');
    div.textContent = String(text);
    return div.innerHTML;
}

// Synchronisation automatique de l'heure depuis le téléphone.
// Le Raspberry Pi n'a ni horloge sauvegardée ni internet sur son hotspot :
// sans cela, la plage horaire de capture serait fausse. Le serveur ignore
// les petits écarts et ne touche jamais l'horloge pendant une capture.
async function aurionSyncClock() {
    try {
        let timezone = null;
        try { timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || null; } catch (_) { }
        const res = await fetch('/api/system/time', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ epoch_ms: Date.now(), timezone, auto: true }),
        });
        if (!res.ok) return;
        const data = await res.json();
        if (data.changed && typeof showToast === 'function') showToast("Heure synchronisée avec le téléphone", 'success');
        // The clock is now confirmed (changed or already right): refresh checks
        document.dispatchEvent(new Event('aurion-clock'));
    } catch (_) { /* hors ligne : on réessaiera au prochain chargement */ }
}
// Every page load: the Pi may have rebooted since (no saved clock on a Pi 4).
// Small drifts are ignored by the server, never during a night.
aurionSyncClock();

// ─── Menu commun et mode expert ────────────────────────────
// Le menu est généré ici, une seule fois pour toutes les pages.
// Mode simple : l'essentiel pour poser Aurion et récupérer ses photos.
// Mode expert : réglages fins, presets, stockage, diagnostics.
const AURION_MENU = [
    { href: '/index.html', icon: '🌌', label: 'Accueil' },
    { href: '/gallery.html', icon: '📸', label: 'Photos' },
    { href: '/preview.html', icon: '📷', label: 'Cadrage (aperçu)' },
    { href: '/settings.html', icon: '⚙️', label: 'Réglages photo' },
    { href: '/settings_advanced.html', icon: '🔧', label: 'Réglages experts', expert: true },
    { href: '/presets.html', icon: '💾', label: 'Presets', expert: true },
    { href: '/storage.html', icon: '💿', label: 'Stockage', expert: true },
    { href: '/diagnostics.html', icon: '📋', label: 'Diagnostics et mise à jour' },
];

function aurionIsExpert() {
    try { return localStorage.getItem('aurionExpert') === '1'; } catch (_) { return false; }
}

function aurionSetExpert(on) {
    try { localStorage.setItem('aurionExpert', on ? '1' : '0'); } catch (_) { }
    document.documentElement.classList.toggle('expert', on);
    aurionBuildMenu();
}

function aurionBuildMenu() {
    const menu = document.getElementById('sideMenu');
    if (!menu) return;
    const expert = aurionIsExpert();
    const here = location.pathname === '/' ? '/index.html' : location.pathname;
    menu.innerHTML = '';
    for (const item of AURION_MENU) {
        if (item.expert && !expert && here !== item.href) continue;
        const a = document.createElement('a');
        a.href = item.href;
        if (item.href === here) a.className = 'active';
        const icon = document.createElement('span');
        icon.className = 'menu-icon';
        icon.textContent = item.icon;
        a.append(icon, ' ' + item.label);
        menu.appendChild(a);
    }
    const label = document.createElement('label');
    label.className = 'menu-expert';
    const box = document.createElement('input');
    box.type = 'checkbox';
    box.id = 'expertToggle';
    box.checked = expert;
    box.addEventListener('change', () => aurionSetExpert(box.checked));
    label.append(box, ' Mode expert');
    menu.appendChild(label);
}

function toggleMenu() {
    document.getElementById('sideMenu').classList.toggle('open');
    document.getElementById('menuOverlay').classList.toggle('visible');
}

// Rafraîchissement périodique suspendu quand l'écran est éteint ou l'onglet
// caché : moins de requêtes, donc moins de réveils du Pi et du téléphone.
function aurionPoll(fn, ms) {
    setInterval(() => { if (!document.hidden) fn(); }, ms);
    document.addEventListener('visibilitychange', () => { if (!document.hidden) fn(); });
}

if (aurionIsExpert()) document.documentElement.classList.add('expert');
document.addEventListener('DOMContentLoaded', aurionBuildMenu);

// « Préparer la clé » : efface la clé USB et la formate en exFAT (après
// confirmation, avec le modèle et la taille de la clé pour éviter l'erreur).
async function aurionPrepareUsbKey(onDone) {
    try {
        const info = await (await apiFetch('/api/storage/usb')).json();
        const keys = info.devices || [];
        if (keys.length === 0) { showToast('Aucune clé USB détectée : branchez-la puis réessayez', 'error'); return; }
        if (keys.length > 1) { showToast('Branchez une seule clé USB pendant la préparation', 'error'); return; }
        const k = keys[0];
        const size = (k.size_bytes / 1e9).toFixed(0);
        if (!confirm(`Préparer la clé ${k.model || k.name} (${size} Go) ?\n\nTOUT son contenu sera effacé, puis elle sera formatée pour Aurion (exFAT). Cela prend moins d'une minute.`)) return;
        showToast('Préparation de la clé…', 'success');
        await apiFetch('/api/storage/format', {
            method: 'POST', headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ device: k.name, confirm: 'EFFACER' }),
        });
        showToast('Clé prête', 'success');
        if (onDone) onDone();
    } catch (e) {
        showToast(e.message, 'error');
    }
}
