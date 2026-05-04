// MaeclyPrint brand foundation
// Refined utility aesthetic — quiet, technical, monospace accents.
// Single source of truth for color, type, motion across all screens.

const MP_TOKENS = {
  // Light theme — warm-neutral, slightly papery
  light: {
    bg:        '#FAF9F6',          // page
    surface:   '#FFFFFF',          // cards, modals
    surface2:  '#F3F1EC',          // sunken / sidebar
    surface3:  '#EAE7E0',          // hover row
    border:    '#E2DED5',
    borderStrong: '#C9C3B7',
    text:      '#1A1814',
    textMuted: '#6B665D',
    textDim:   '#9A9489',
    accent:    '#C2410C',          // ink red-orange — sparingly
    accentSoft:'#FDEBD3',
    success:   '#3F6212',
    successSoft: '#E5EFD2',
    warn:      '#A16207',
    warnSoft:  '#FAEFD0',
    error:     '#9F1239',
    errorSoft: '#FBE2E8',
    info:      '#0E7490',
    infoSoft:  '#D9EEF1',
    focus:     '#C2410C',
  },
  // Dark — warm graphite, not blue-black
  dark: {
    bg:        '#15140F',
    surface:   '#1C1A15',
    surface2:  '#23211B',
    surface3:  '#2D2A23',
    border:    '#33302A',
    borderStrong: '#4A4640',
    text:      '#F0EDE5',
    textMuted: '#A8A296',
    textDim:   '#6F6A60',
    accent:    '#F97316',
    accentSoft:'#3A1F0E',
    success:   '#9CCB4A',
    successSoft: '#1F2A12',
    warn:      '#E0B341',
    warnSoft:  '#2D2412',
    error:     '#F87171',
    errorSoft: '#2D1418',
    info:      '#4FC1D7',
    infoSoft:  '#0E2429',
    focus:     '#F97316',
  },
};

const MP_FONTS = {
  // System UI for chrome (cross-OS native feel) + JetBrains Mono for technical surfaces
  ui:   "ui-sans-serif, -apple-system, 'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif",
  mono: "'JetBrains Mono', 'SF Mono', 'Cascadia Code', 'Consolas', ui-monospace, monospace",
  display: "ui-sans-serif, -apple-system, 'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif",
};

// Inject CSS variables + base styles + font import once.
if (typeof document !== 'undefined' && !document.getElementById('mp-tokens')) {
  // JetBrains Mono via Google Fonts
  if (!document.querySelector('link[data-mp-font]')) {
    const pre = document.createElement('link');
    pre.rel = 'preconnect'; pre.href = 'https://fonts.gstatic.com'; pre.crossOrigin = '';
    document.head.appendChild(pre);
    const fl = document.createElement('link');
    fl.dataset.mpFont = '1';
    fl.rel = 'stylesheet';
    fl.href = 'https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;600&display=swap';
    document.head.appendChild(fl);
  }
  const s = document.createElement('style');
  s.id = 'mp-tokens';
  const lvars = Object.entries(MP_TOKENS.light).map(([k,v])=>`--mp-${k}:${v}`).join(';');
  const dvars = Object.entries(MP_TOKENS.dark ).map(([k,v])=>`--mp-${k}:${v}`).join(';');
  s.textContent = `
    .mp-light{${lvars};--mp-font-ui:${MP_FONTS.ui};--mp-font-mono:${MP_FONTS.mono}}
    .mp-dark{${dvars};--mp-font-ui:${MP_FONTS.ui};--mp-font-mono:${MP_FONTS.mono}}
    .mp-app *{box-sizing:border-box}
    .mp-app{font-family:var(--mp-font-ui);color:var(--mp-text);background:var(--mp-bg);
      font-feature-settings:"ss01","cv11";letter-spacing:-0.005em;-webkit-font-smoothing:antialiased}
    .mp-mono{font-family:var(--mp-font-mono);font-feature-settings:"calt","liga"}
    .mp-app button{font-family:inherit;color:inherit}
    .mp-app input,.mp-app select,.mp-app textarea{font-family:inherit;color:inherit}
    .mp-scroll::-webkit-scrollbar{width:10px;height:10px}
    .mp-scroll::-webkit-scrollbar-track{background:transparent}
    .mp-scroll::-webkit-scrollbar-thumb{background:var(--mp-border);border-radius:6px;border:2px solid transparent;background-clip:content-box}
    .mp-scroll::-webkit-scrollbar-thumb:hover{background:var(--mp-borderStrong);background-clip:content-box;border:2px solid transparent}
    @keyframes mp-spin{to{transform:rotate(360deg)}}
    @keyframes mp-pulse{0%,100%{opacity:.4}50%{opacity:1}}
    @keyframes mp-fadein{from{opacity:0;transform:translateY(4px)}to{opacity:1;transform:none}}
    @keyframes mp-slidein{from{opacity:0;transform:translateX(8px)}to{opacity:1;transform:none}}
    @keyframes mp-progress{0%{transform:translateX(-100%)}100%{transform:translateX(400%)}}
    .mp-focus:focus-visible{outline:2px solid var(--mp-focus);outline-offset:2px}
  `;
  document.head.appendChild(s);
}

// ─────────────────────────────────────────────────────────────
// Logo / wordmark
// ─────────────────────────────────────────────────────────────
function MPGlyph({ size = 24, color, bg }) {
  // Stylized "M" formed by stacked paper sheets meeting an accent stroke.
  // Two rectangles offset, with a diagonal accent representing the "print pass".
  const s = size;
  const c = color || 'currentColor';
  const b = bg || 'var(--mp-bg)';
  return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="2.5" y="3.5" width="13" height="17" rx="1.5" fill={b} stroke={c} strokeWidth="1.5"/>
      <rect x="8.5" y="3.5" width="13" height="17" rx="1.5" fill={b} stroke={c} strokeWidth="1.5"/>
      <path d="M11 8 L15 13 L19 8" stroke="var(--mp-accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" fill="none"/>
      <line x1="11" y1="16" x2="19" y2="16" stroke={c} strokeWidth="1.2" strokeLinecap="round" opacity="0.5"/>
    </svg>
  );
}

function MPWordmark({ size = 16, showGlyph = true }) {
  return (
    <span style={{ display:'inline-flex', alignItems:'center', gap: size*0.4 }}>
      {showGlyph && <MPGlyph size={size*1.3} />}
      <span style={{
        fontFamily: MP_FONTS.ui,
        fontSize: size,
        fontWeight: 600,
        letterSpacing: '-0.02em',
        color: 'var(--mp-text)',
      }}>
        Maecly<span style={{ color:'var(--mp-accent)' }}>Print</span>
      </span>
    </span>
  );
}

// App icon — for installer / Start menu / dock. Includes background.
function MPAppIcon({ size = 64, rounded = true }) {
  const s = size;
  return (
    <div style={{
      width: s, height: s, borderRadius: rounded ? s*0.22 : 0,
      background: 'linear-gradient(155deg, #FAF9F6 0%, #ECE7DC 100%)',
      boxShadow: `inset 0 0 0 1px rgba(0,0,0,0.08), 0 ${s*0.04}px ${s*0.12}px rgba(0,0,0,0.18)`,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      position: 'relative', overflow:'hidden'
    }}>
      <svg width={s*0.62} height={s*0.62} viewBox="0 0 24 24" fill="none">
        <rect x="2.5" y="3.5" width="13" height="17" rx="1.5" fill="#FFFFFF" stroke="#1A1814" strokeWidth="1.4"/>
        <rect x="8.5" y="3.5" width="13" height="17" rx="1.5" fill="#FFFFFF" stroke="#1A1814" strokeWidth="1.4"/>
        <path d="M11 8 L15 13 L19 8" stroke="#C2410C" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" fill="none"/>
        <line x1="11" y1="16" x2="19" y2="16" stroke="#1A1814" strokeWidth="1.2" strokeLinecap="round" opacity="0.5"/>
      </svg>
      <div style={{
        position:'absolute', inset:0, pointerEvents:'none',
        background:'radial-gradient(120% 80% at 30% 0%, rgba(255,255,255,0.7), transparent 60%)'
      }}/>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// Tiny icon set — feather-style, 1.5 stroke. Inline so we never miss icons.
// ─────────────────────────────────────────────────────────────
function Icon({ name, size = 16, stroke = 1.5, color = 'currentColor' }) {
  const p = { width:size, height:size, viewBox:'0 0 24 24', fill:'none',
    stroke:color, strokeWidth:stroke, strokeLinecap:'round', strokeLinejoin:'round' };
  switch (name) {
    case 'printer': return <svg {...p}><path d="M6 9V3h12v6"/><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"/><rect x="6" y="14" width="12" height="8" rx="1"/></svg>;
    case 'file':    return <svg {...p}><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="14 3 14 9 20 9"/></svg>;
    case 'folder':  return <svg {...p}><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>;
    case 'plus':    return <svg {...p}><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>;
    case 'check':   return <svg {...p}><polyline points="20 6 9 17 4 12"/></svg>;
    case 'x':       return <svg {...p}><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>;
    case 'play':    return <svg {...p}><polygon points="6 4 20 12 6 20 6 4" fill={color}/></svg>;
    case 'pause':   return <svg {...p}><rect x="6" y="4" width="4" height="16" fill={color}/><rect x="14" y="4" width="4" height="16" fill={color}/></svg>;
    case 'rotate':  return <svg {...p}><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>;
    case 'trash':   return <svg {...p}><polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/></svg>;
    case 'settings':return <svg {...p}><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>;
    case 'home':    return <svg {...p}><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>;
    case 'list':    return <svg {...p}><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>;
    case 'queue':   return <svg {...p}><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="13" y2="18"/></svg>;
    case 'grid':    return <svg {...p}><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>;
    case 'user':    return <svg {...p}><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>;
    case 'info':    return <svg {...p}><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>;
    case 'alert':   return <svg {...p}><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>;
    case 'chevr':   return <svg {...p}><polyline points="9 18 15 12 9 6"/></svg>;
    case 'chevd':   return <svg {...p}><polyline points="6 9 12 15 18 9"/></svg>;
    case 'chevu':   return <svg {...p}><polyline points="18 15 12 9 6 15"/></svg>;
    case 'chevl':   return <svg {...p}><polyline points="15 18 9 12 15 6"/></svg>;
    case 'arrow-r': return <svg {...p}><line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/></svg>;
    case 'download':return <svg {...p}><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>;
    case 'upload': return <svg {...p}><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>;
    case 'sun':     return <svg {...p}><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/></svg>;
    case 'moon':    return <svg {...p}><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>;
    case 'sliders': return <svg {...p}><line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/></svg>;
    case 'copy':    return <svg {...p}><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>;
    case 'edit':    return <svg {...p}><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>;
    case 'star':    return <svg {...p}><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>;
    case 'lock':    return <svg {...p}><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>;
    case 'shield':  return <svg {...p}><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>;
    case 'dot':     return <svg {...p}><circle cx="12" cy="12" r="3" fill={color}/></svg>;
    case 'more':    return <svg {...p}><circle cx="12" cy="12" r="1" fill={color}/><circle cx="19" cy="12" r="1" fill={color}/><circle cx="5" cy="12" r="1" fill={color}/></svg>;
    case 'search':  return <svg {...p}><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>;
    case 'cmd':     return <svg {...p}><path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3z"/></svg>;
    case 'help':    return <svg {...p}><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>;
    case 'image':   return <svg {...p}><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>;
    case 'page':    return <svg {...p}><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="14 3 14 9 20 9"/><line x1="8" y1="13" x2="16" y2="13"/><line x1="8" y1="17" x2="13" y2="17"/></svg>;
    case 'palette': return <svg {...p}><circle cx="13.5" cy="6.5" r=".5" fill={color}/><circle cx="17.5" cy="10.5" r=".5" fill={color}/><circle cx="8.5" cy="7.5" r=".5" fill={color}/><circle cx="6.5" cy="12.5" r=".5" fill={color}/><path d="M12 22a10 10 0 1 1 0-20 10 10 0 0 1 10 10c0 1.66-.74 3-2 3h-1.5a1.5 1.5 0 0 0-1.5 1.5 2 2 0 0 1-2 2H12z"/></svg>;
    case 'cut':     return <svg {...p}><circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><line x1="20" y1="4" x2="8.12" y2="15.88"/><line x1="14.47" y1="14.48" x2="20" y2="20"/><line x1="8.12" y1="8.12" x2="12" y2="12"/></svg>;
    case 'rename':  return <svg {...p}><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 1 1 3 3L7 19l-4 1 1-4z"/></svg>;
    case 'share':   return <svg {...p}><circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" y1="13.51" x2="15.42" y2="17.49"/><line x1="15.41" y1="6.51" x2="8.59" y2="10.49"/></svg>;
    case 'archive': return <svg {...p}><polyline points="21 8 21 21 3 21 3 8"/><rect x="1" y="3" width="22" height="5"/><line x1="10" y1="12" x2="14" y2="12"/></svg>;
    case 'openwith':return <svg {...p}><path d="M14 3h7v7"/><path d="M5 21l16-16"/><path d="M21 14v7H3V3h7"/></svg>;
    case 'dots':    return <svg {...p}><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></svg>;
    case 'arrowl':  return <svg {...p}><line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 19 5 12 12 5"/></svg>;
    case 'arrowr':  return <svg {...p}><line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/></svg>;
    case 'arrowu':  return <svg {...p}><line x1="12" y1="19" x2="12" y2="5"/><polyline points="5 12 12 5 19 12"/></svg>;
    case 'maecly':  return <svg {...p}><rect x="2.5" y="3.5" width="13" height="17" rx="1.5"/><rect x="8.5" y="3.5" width="13" height="17" rx="1.5"/><path d="M11 8 L15 13 L19 8" stroke="var(--mp-accent)"/></svg>;
    case 'apple':   return <svg width={size} height={size} viewBox="0 0 24 24" fill={color}><path d="M17.05 20.28c-.98.95-2.05.8-3.08.35-1.09-.46-2.09-.48-3.24 0-1.44.62-2.2.44-3.06-.35C2.79 15.25 3.51 7.59 9.05 7.31c1.35.07 2.29.74 3.08.8 1.18-.24 2.31-.93 3.57-.84 1.51.12 2.65.72 3.4 1.8-3.12 1.87-2.38 5.98.48 7.13-.57 1.5-1.31 2.99-2.54 4.09zM12.03 7.25c-.15-2.23 1.66-4.07 3.74-4.25.29 2.58-2.34 4.5-3.74 4.25z"/></svg>;
    case 'windows': return <svg width={size} height={size} viewBox="0 0 24 24" fill={color}><path d="M0 3.449L9.75 2.1v9.451H0m10.949-9.602L24 0v11.4H10.949M0 12.6h9.75v9.451L0 20.699M10.949 12.6H24V24l-13.051-1.819"/></svg>;
    default: return null;
  }
}

Object.assign(window, { MP_TOKENS, MP_FONTS, MPGlyph, MPWordmark, MPAppIcon, Icon });
