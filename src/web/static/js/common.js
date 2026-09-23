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
(async function aurionSyncClock() {
    try {
        if (sessionStorage.getItem('aurionClockSynced')) return;
    } catch (_) { /* navigation privée */ }
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
        if (data.changed && typeof showToast === 'function') {
            showToast("Heure synchronisée avec le téléphone", 'success');
        }
        try { sessionStorage.setItem('aurionClockSynced', '1'); } catch (_) { }
    } catch (_) { /* hors ligne : on réessaiera au prochain chargement */ }
})();
