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

// ─── Résultat d'une mise à jour ────────────────────────────
// Après une mise à jour, le téléphone se reconnecte au Wi-Fi Aurion : la
// première page ouverte affiche le résultat (validée ou retour arrière), avec
// le nombre d'avertissements et le lien vers le journal. Pendant les minutes
// d'essai, un bandeau l'annonce et la page vérifie toute seule.
let aurionUpdateTimer = null;
async function aurionUpdateResult() {
    let u;
    try { u = await (await fetch('/api/system/update/status')).json(); } catch (_) { return; }
    const last = (u && u.last) || {};
    const banner = document.getElementById('aurionUpdateBanner');
    if (last.outcome === 'essai') {
        if (!banner) {
            const b = document.createElement('div');
            b.id = 'aurionUpdateBanner';
            b.setAttribute('role', 'status');
            b.style.cssText = 'position:fixed;left:0;right:0;bottom:0;z-index:900;padding:0.6rem 1rem;'
                + 'background:var(--bg-card);border-top:1px solid var(--border);color:var(--text-primary);'
                + 'font-size:0.85rem;text-align:center;';
            b.textContent = `Mise à jour vers ${last.to || 'la nouvelle version'} installée : vérification en cours `
                + '(environ 3 minutes), le résultat s\'affichera ici.';
            document.body.appendChild(b);
        }
        if (!aurionUpdateTimer) aurionUpdateTimer = setInterval(aurionUpdateResult, 20000);
        return;
    }
    if (banner) banner.remove();
    if (aurionUpdateTimer) { clearInterval(aurionUpdateTimer); aurionUpdateTimer = null; }
    if ((last.outcome === 'validee' || last.outcome === 'echec') && !last.seen) aurionShowUpdateResult(u);
}

function aurionShowUpdateResult(u) {
    if (document.getElementById('aurionUpdateDialog')) return;
    const last = u.last || {};
    const ok = last.outcome === 'validee';
    const overlay = document.createElement('div');
    overlay.id = 'aurionUpdateDialog';
    overlay.setAttribute('role', 'dialog');
    overlay.setAttribute('aria-modal', 'true');
    overlay.style.cssText = 'position:fixed;inset:0;z-index:1000;display:flex;align-items:center;justify-content:center;'
        + 'padding:1rem;background:rgba(0,0,0,0.6);';
    const counts = [];
    if (u.warnings) counts.push(`${u.warnings} avertissement${u.warnings > 1 ? 's' : ''}`);
    if (u.errors) counts.push(`${u.errors} erreur${u.errors > 1 ? 's' : ''}`);
    const detail = counts.length ? `Journal : ${counts.join(', ')} (non bloquants si la mise à jour est validée).`
        : 'Journal : aucun avertissement ni erreur.';
    const title = ok ? `Mise à jour validée : ${aurionEscape(last.to || u.version || '')}`
        : 'Mise à jour non appliquée';
    const journal = last.log
        ? `<a class="btn btn-outline btn-block" style="margin-top:0.75rem;" href="/api/system/update/journal/${encodeURIComponent(last.log)}" target="_blank" rel="noopener">Voir le journal de mise à jour</a>`
        : '';
    overlay.innerHTML = `<div class="card" style="max-width:26rem;width:100%;margin:0;">
        <div class="card-title" style="margin-bottom:0.5rem;">${ok ? '✅' : '↩️'} ${title}</div>
        <p style="margin:0 0 0.5rem;">${aurionEscape(last.message || '')}</p>
        <p class="muted small" style="margin:0;">${detail}</p>
        ${journal}
        <p class="muted small" style="margin:0.5rem 0 0;">Tous les journaux : Diagnostics, rubrique « Journaux de mise à jour ».</p>
        <button type="button" class="btn btn-primary btn-block" style="margin-top:0.75rem;" id="aurionUpdateOk">OK</button>
    </div>`;
    document.body.appendChild(overlay);
    document.getElementById('aurionUpdateOk').addEventListener('click', async () => {
        overlay.remove();
        try { await fetch('/api/system/update/seen', { method: 'POST' }); } catch (_) { }
    });
}
document.addEventListener('DOMContentLoaded', aurionUpdateResult);
