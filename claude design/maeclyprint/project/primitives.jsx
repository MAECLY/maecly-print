// Shared UI primitives for MaeclyPrint
// Buttons, inputs, selects, badges, kbd, separators, panels.
// Token-driven; no hardcoded colors.

function MPButton({ children, variant='secondary', size='md', icon, iconRight, onClick, disabled, fullWidth, type='button', style }) {
  const sizes = {
    sm: { px: 8,  py: 4,  fs: 12, h: 24, gap: 5, rad: 5 },
    md: { px: 12, py: 6,  fs: 13, h: 30, gap: 6, rad: 6 },
    lg: { px: 16, py: 9,  fs: 14, h: 36, gap: 8, rad: 7 },
  }[size];
  const variants = {
    primary: { bg: 'var(--mp-accent)', fg: '#FFF', bd: 'var(--mp-accent)' },
    secondary: { bg: 'var(--mp-surface)', fg: 'var(--mp-text)', bd: 'var(--mp-border)' },
    ghost: { bg: 'transparent', fg: 'var(--mp-text)', bd: 'transparent' },
    danger: { bg: 'var(--mp-surface)', fg: 'var(--mp-error)', bd: 'var(--mp-border)' },
  }[variant];
  const [hover, setHover] = React.useState(false);
  return (
    <button type={type} onClick={onClick} disabled={disabled}
      onMouseEnter={()=>setHover(true)} onMouseLeave={()=>setHover(false)}
      className="mp-focus"
      style={{
        display: 'inline-flex', alignItems: 'center', justifyContent:'center', gap: sizes.gap,
        padding: `${sizes.py}px ${sizes.px}px`, height: sizes.h,
        fontSize: sizes.fs, fontWeight: 500, lineHeight: 1,
        background: hover && !disabled ? (variant==='primary' ? 'var(--mp-accent)' : variant==='ghost' ? 'var(--mp-surface3)' : 'var(--mp-surface2)') : variants.bg,
        color: variants.fg, border: `1px solid ${variants.bd}`,
        borderRadius: sizes.rad, cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.5 : 1, width: fullWidth ? '100%' : 'auto',
        transition: 'background .12s, border-color .12s, transform .08s',
        boxShadow: variant==='primary' ? '0 1px 0 rgba(0,0,0,0.04), inset 0 1px 0 rgba(255,255,255,0.12)' : 'none',
        whiteSpace: 'nowrap', ...style,
      }}>
      {icon && <Icon name={icon} size={sizes.fs+1} />}
      {children}
      {iconRight && <Icon name={iconRight} size={sizes.fs+1} />}
    </button>
  );
}

function MPIconButton({ icon, onClick, size=28, label, active, disabled }) {
  const [hover, setHover] = React.useState(false);
  return (
    <button onClick={onClick} aria-label={label} disabled={disabled} title={label}
      onMouseEnter={()=>setHover(true)} onMouseLeave={()=>setHover(false)}
      className="mp-focus"
      style={{
        width: size, height: size, display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        background: active ? 'var(--mp-surface3)' : (hover && !disabled ? 'var(--mp-surface2)' : 'transparent'),
        color: active ? 'var(--mp-text)' : 'var(--mp-textMuted)',
        border: 'none', borderRadius: 6, cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.4 : 1, transition: 'background .12s, color .12s',
      }}>
      <Icon name={icon} size={Math.round(size*0.55)} />
    </button>
  );
}

function MPKbd({ children }) {
  return (
    <span className="mp-mono" style={{
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      minWidth: 18, height: 18, padding: '0 5px',
      fontSize: 10.5, fontWeight: 500,
      background: 'var(--mp-surface2)', color: 'var(--mp-textMuted)',
      border: '1px solid var(--mp-border)', borderRadius: 4,
    }}>{children}</span>
  );
}

function MPBadge({ children, tone = 'neutral', icon }) {
  const tones = {
    neutral: { bg: 'var(--mp-surface2)', fg: 'var(--mp-textMuted)', bd: 'var(--mp-border)' },
    success: { bg: 'var(--mp-successSoft)', fg: 'var(--mp-success)', bd: 'transparent' },
    warn:    { bg: 'var(--mp-warnSoft)', fg: 'var(--mp-warn)', bd: 'transparent' },
    error:   { bg: 'var(--mp-errorSoft)', fg: 'var(--mp-error)', bd: 'transparent' },
    info:    { bg: 'var(--mp-infoSoft)', fg: 'var(--mp-info)', bd: 'transparent' },
    accent:  { bg: 'var(--mp-accentSoft)', fg: 'var(--mp-accent)', bd: 'transparent' },
  }[tone];
  return (
    <span className="mp-mono" style={{
      display: 'inline-flex', alignItems: 'center', gap: 4,
      padding: '2px 6px', fontSize: 10.5, fontWeight: 500,
      background: tones.bg, color: tones.fg, border: `1px solid ${tones.bd}`,
      borderRadius: 4, textTransform: 'uppercase', letterSpacing: '0.04em',
    }}>
      {icon && <Icon name={icon} size={11} />}
      {children}
    </span>
  );
}

function MPInput({ value, onChange=()=>{}, placeholder, type='text', size='md', mono, fullWidth, suffix, prefix, autoFocus, onKeyDown, error }) {
  const sizes = { sm:{h:24,fs:12,px:8}, md:{h:30,fs:13,px:10}, lg:{h:36,fs:14,px:12} }[size];
  const [focus, setFocus] = React.useState(false);
  return (
    <div style={{
      display:'inline-flex', alignItems:'center', gap: 6,
      padding: `0 ${sizes.px}px`, height: sizes.h,
      background: 'var(--mp-surface)',
      border: `1px solid ${error ? 'var(--mp-error)' : (focus ? 'var(--mp-focus)' : 'var(--mp-border)')}`,
      borderRadius: 6, width: fullWidth ? '100%' : 'auto',
      transition: 'border-color .12s, box-shadow .12s',
      boxShadow: focus ? '0 0 0 3px var(--mp-accentSoft)' : 'none',
    }}>
      {prefix && <span style={{color:'var(--mp-textMuted)', display:'flex'}}>{prefix}</span>}
      <input value={value} onChange={onChange} placeholder={placeholder} type={type}
        autoFocus={autoFocus} onKeyDown={onKeyDown}
        onFocus={()=>setFocus(true)} onBlur={()=>setFocus(false)}
        className={mono ? 'mp-mono' : ''}
        style={{
          flex:1, height:'100%', border:'none', outline:'none', background:'transparent',
          fontSize: sizes.fs, color:'var(--mp-text)', minWidth: 0,
        }} />
      {suffix && <span style={{color:'var(--mp-textMuted)', fontSize:sizes.fs-1}}>{suffix}</span>}
    </div>
  );
}

function MPSelect({ value, onChange=()=>{}, options, size='md', fullWidth }) {
  const sizes = { sm:{h:24,fs:12,px:8}, md:{h:30,fs:13,px:10}, lg:{h:36,fs:14,px:12} }[size];
  return (
    <div style={{ position:'relative', display:'inline-block', width: fullWidth?'100%':'auto' }}>
      <select value={value} onChange={onChange}
        style={{
          appearance:'none', WebkitAppearance:'none',
          height: sizes.h, padding: `0 ${sizes.px+18}px 0 ${sizes.px}px`,
          fontSize: sizes.fs, color: 'var(--mp-text)',
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)', borderRadius: 6,
          cursor: 'pointer', width: fullWidth?'100%':'auto', outline:'none',
        }}>
        {options.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
      </select>
      <span style={{ position:'absolute', right: 8, top:'50%', transform:'translateY(-50%)', pointerEvents:'none', display:'flex', color:'var(--mp-textMuted)' }}>
        <Icon name="chevd" size={13}/>
      </span>
    </div>
  );
}

function MPSegmented({ value, onChange, options, size='md' }) {
  const h = size==='sm' ? 24 : size==='lg' ? 36 : 28;
  return (
    <div style={{
      display: 'inline-flex', padding: 2, height: h+4,
      background: 'var(--mp-surface2)', border: '1px solid var(--mp-border)', borderRadius: 7,
    }}>
      {options.map(o => {
        const active = o.value === value;
        return (
          <button key={o.value} onClick={()=>onChange(o.value)}
            className="mp-focus"
            style={{
              height: h, padding: `0 ${size==='sm'?8:12}px`,
              fontSize: size==='sm'?11.5:12.5, fontWeight: active?500:400,
              background: active ? 'var(--mp-surface)' : 'transparent',
              color: active ? 'var(--mp-text)' : 'var(--mp-textMuted)',
              border:'none', borderRadius: 5, cursor:'pointer',
              boxShadow: active ? '0 1px 2px rgba(0,0,0,0.05), 0 0 0 1px var(--mp-border)' : 'none',
              display:'inline-flex', alignItems:'center', gap: 5,
              transition:'background .12s, color .12s',
              whiteSpace:'nowrap',
            }}>
            {o.icon && <Icon name={o.icon} size={13}/>}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

function MPCheckbox({ checked, onChange, label, indeterminate }) {
  return (
    <label style={{ display:'inline-flex', alignItems:'center', gap:8, cursor:'pointer', fontSize:13, color:'var(--mp-text)' }}>
      <span style={{
        width: 15, height: 15, display:'inline-flex', alignItems:'center', justifyContent:'center',
        background: checked||indeterminate ? 'var(--mp-accent)' : 'var(--mp-surface)',
        border: `1px solid ${checked||indeterminate ? 'var(--mp-accent)' : 'var(--mp-borderStrong)'}`,
        borderRadius: 3.5, transition: 'all .12s',
      }}>
        {checked && <Icon name="check" size={11} color="#FFF" stroke={2.5}/>}
        {indeterminate && !checked && <span style={{width:7, height:1.5, background:'#FFF', borderRadius:1}}/>}
      </span>
      <input type="checkbox" checked={checked} onChange={onChange} style={{display:'none'}}/>
      {label}
    </label>
  );
}

function MPSwitch({ checked, onChange, label }) {
  return (
    <label style={{ display:'inline-flex', alignItems:'center', gap:10, cursor:'pointer', fontSize:13 }}>
      <span style={{
        width: 30, height: 17, padding: 2, position:'relative',
        background: checked ? 'var(--mp-accent)' : 'var(--mp-surface3)',
        border: `1px solid ${checked ? 'var(--mp-accent)' : 'var(--mp-borderStrong)'}`,
        borderRadius: 99, transition:'background .15s',
      }}>
        <span style={{
          position:'absolute', top:1.5, left: checked ? 14 : 1.5, width: 13, height: 13,
          background: '#FFF', borderRadius:'50%', transition:'left .15s',
          boxShadow:'0 1px 2px rgba(0,0,0,0.2)',
        }}/>
      </span>
      <input type="checkbox" checked={checked} onChange={onChange} style={{display:'none'}}/>
      {label && <span style={{color:'var(--mp-text)'}}>{label}</span>}
    </label>
  );
}

function MPDivider({ vertical, mono }) {
  if (vertical) return <span style={{ width:1, alignSelf:'stretch', background:'var(--mp-border)' }}/>;
  return <div style={{ height:1, background:'var(--mp-border)', width:'100%' }}/>;
}

function MPCard({ children, padding=16, style, onClick, hoverable }) {
  const [hover, setHover] = React.useState(false);
  return (
    <div onClick={onClick}
      onMouseEnter={()=>setHover(true)} onMouseLeave={()=>setHover(false)}
      style={{
        background:'var(--mp-surface)', border:'1px solid var(--mp-border)',
        borderRadius: 8, padding,
        boxShadow: hover && hoverable ? '0 2px 6px rgba(0,0,0,0.04)' : 'none',
        cursor: onClick ? 'pointer' : 'default',
        transition:'box-shadow .15s, border-color .12s',
        borderColor: hover && hoverable ? 'var(--mp-borderStrong)' : 'var(--mp-border)',
        ...style,
      }}>
      {children}
    </div>
  );
}

function MPSectionLabel({ children, mono = true }) {
  return (
    <div className={mono ? 'mp-mono' : ''} style={{
      fontSize: 10.5, fontWeight: 500, letterSpacing: '0.08em',
      color: 'var(--mp-textDim)', textTransform: 'uppercase', marginBottom: 8,
    }}>{children}</div>
  );
}

// File-type icon — colored swatch with extension text
function FileGlyph({ ext, size = 30 }) {
  const colors = {
    pdf:  ['#FBE2E8', '#9F1239'],
    docx: ['#D9EEF1', '#0E7490'], doc: ['#D9EEF1', '#0E7490'],
    xlsx: ['#E5EFD2', '#3F6212'], xls: ['#E5EFD2', '#3F6212'],
    pptx: ['#FAEFD0', '#A16207'], ppt: ['#FAEFD0', '#A16207'],
    jpg:  ['#FDEBD3', '#C2410C'], jpeg: ['#FDEBD3', '#C2410C'], png: ['#FDEBD3', '#C2410C'], heic:['#FDEBD3', '#C2410C'], tiff:['#FDEBD3','#C2410C'],
    txt:  ['#F0EDE5', '#6B665D'],
    md:   ['#E8E2F4', '#5B21B6'], markdown:['#E8E2F4','#5B21B6'],
    html: ['#FAEFD0', '#A16207'],
    odt:  ['#D9EEF1', '#0E7490'],
  };
  const [bg, fg] = colors[ext.toLowerCase()] || ['#F0EDE5', '#6B665D'];
  return (
    <div style={{
      width: size, height: size*1.25, position:'relative', flexShrink:0,
      background: bg, borderRadius: 3,
      boxShadow:'inset 0 0 0 1px rgba(0,0,0,0.06)',
      display:'flex', alignItems:'flex-end', justifyContent:'center', paddingBottom: 3,
    }}>
      <div style={{
        position:'absolute', top:0, right:0, width: size*0.32, height: size*0.32,
        background: 'var(--mp-bg)', clipPath:'polygon(0 0, 100% 100%, 100% 0)',
      }}/>
      <span className="mp-mono" style={{
        fontSize: Math.max(7.5, size*0.26), fontWeight: 600, color: fg,
        textTransform:'uppercase', letterSpacing:'0.04em',
      }}>{ext}</span>
    </div>
  );
}

// Status dot with optional pulse
function StatusDot({ status }) {
  const cfg = {
    queued:    { c: 'var(--mp-textDim)' },
    converting:{ c: 'var(--mp-info)', pulse:true },
    converted: { c: 'var(--mp-info)' },
    printing:  { c: 'var(--mp-accent)', pulse:true },
    done:      { c: 'var(--mp-success)' },
    failed:    { c: 'var(--mp-error)' },
    paused:    { c: 'var(--mp-warn)' },
    cancelled: { c: 'var(--mp-textDim)' },
  }[status] || { c: 'var(--mp-textDim)' };
  return (
    <span style={{
      display:'inline-block', width: 7, height: 7, borderRadius:'50%',
      background: cfg.c, flexShrink:0,
      animation: cfg.pulse ? 'mp-pulse 1.4s ease-in-out infinite' : 'none',
      boxShadow: cfg.pulse ? `0 0 0 3px ${cfg.c}22` : 'none',
    }}/>
  );
}

Object.assign(window, {
  MPButton, MPIconButton, MPKbd, MPBadge, MPInput, MPSelect, MPSegmented,
  MPCheckbox, MPSwitch, MPDivider, MPCard, MPSectionLabel,
  FileGlyph, StatusDot
});
