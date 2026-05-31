// MaeclyPrint main app — Windows 11 chrome + all screens.
// Self-contained: rendered inside a fixed-size frame on the design canvas.

function MPWinChrome({ children, theme='light', os='windows', title='MaeclyPrint', density='comfortable' }) {
  return (
    <div className={`mp-app mp-${theme}`} style={{
      width:'100%', height:'100%', display:'flex', flexDirection:'column',
      background:'var(--mp-bg)', color:'var(--mp-text)',
      borderRadius: os==='macos' ? 10 : 8, overflow:'hidden',
      boxShadow:'0 0 0 1px rgba(0,0,0,0.06), 0 20px 50px -10px rgba(0,0,0,0.18)',
    }}>
      {/* Title bar */}
      <div style={{
        display:'flex', alignItems:'center', justifyContent:'space-between',
        height: os==='macos' ? 36 : 32,
        background: theme==='dark' ? 'var(--mp-surface)' : 'var(--mp-surface)',
        borderBottom:'1px solid var(--mp-border)',
        padding: os==='macos' ? '0 12px' : '0 0 0 12px',
        flexShrink:0, userSelect:'none',
      }}>
        {os==='macos' ? (
          <div style={{display:'flex', gap:8, alignItems:'center'}}>
            <span style={{width:12,height:12,borderRadius:'50%',background:'#ED6A5E',border:'1px solid rgba(0,0,0,0.08)'}}/>
            <span style={{width:12,height:12,borderRadius:'50%',background:'#F5BF4F',border:'1px solid rgba(0,0,0,0.08)'}}/>
            <span style={{width:12,height:12,borderRadius:'50%',background:'#62C554',border:'1px solid rgba(0,0,0,0.08)'}}/>
          </div>
        ) : (
          <div style={{display:'flex', alignItems:'center', gap:8}}>
            <MPGlyph size={14}/>
            <span style={{fontSize:12, color:'var(--mp-textMuted)', fontWeight:500}}>{title}</span>
          </div>
        )}
        <div style={{flex:1, textAlign:'center', fontSize:12, color:'var(--mp-textMuted)', fontWeight:500}}>
          {os==='macos' ? title : ''}
        </div>
        {os==='windows' ? (
          <div style={{display:'flex', height:'100%'}}>
            <span style={{display:'flex',alignItems:'center',justifyContent:'center',width:46,height:'100%',color:'var(--mp-textMuted)'}}>
              <svg width="10" height="1" viewBox="0 0 10 1"><line x1="0" y1="0.5" x2="10" y2="0.5" stroke="currentColor"/></svg>
            </span>
            <span style={{display:'flex',alignItems:'center',justifyContent:'center',width:46,height:'100%',color:'var(--mp-textMuted)'}}>
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor"/></svg>
            </span>
            <span style={{display:'flex',alignItems:'center',justifyContent:'center',width:46,height:'100%',color:'var(--mp-textMuted)'}}>
              <svg width="10" height="10" viewBox="0 0 10 10"><line x1="0" y1="0" x2="10" y2="10" stroke="currentColor"/><line x1="10" y1="0" x2="0" y2="10" stroke="currentColor"/></svg>
            </span>
          </div>
        ) : <div style={{width:50}}/>}
      </div>
      {children}
    </div>
  );
}

function MPSidebar({ active, onChange, queueCount=4 }) {
  const items = [
    { id:'home', icon:'home', label:'Home' },
    { id:'job', icon:'file', label:'Print Job', kbd:'⌘P' },
    { id:'queue', icon:'queue', label:'Queue', count: queueCount },
    { id:'printers', icon:'printer', label:'Printers' },
    { id:'profiles', icon:'sliders', label:'Profiles' },
    { id:'settings', icon:'settings', label:'Settings' },
    { id:'about', icon:'info', label:'About' },
  ];
  return (
    <div style={{
      width: 200, flexShrink:0, background:'var(--mp-surface2)',
      borderRight:'1px solid var(--mp-border)',
      display:'flex', flexDirection:'column', padding:'10px 8px',
    }}>
      <div style={{padding:'6px 8px 12px'}}>
        <MPWordmark size={13}/>
      </div>
      <nav style={{display:'flex', flexDirection:'column', gap:1}}>
        {items.map(it => {
          const a = it.id===active;
          return (
            <button key={it.id} onClick={()=>onChange?.(it.id)}
              className="mp-focus"
              style={{
                display:'flex', alignItems:'center', gap:9,
                padding:'6px 9px', height:30,
                background: a ? 'var(--mp-surface)' : 'transparent',
                color: a ? 'var(--mp-text)' : 'var(--mp-textMuted)',
                border:'none', borderRadius:6, cursor:'pointer',
                fontSize:13, fontWeight: a?500:400, textAlign:'left',
                boxShadow: a ? '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px var(--mp-border)' : 'none',
                transition:'background .12s, color .12s',
              }}>
              <Icon name={it.icon} size={15}/>
              <span style={{flex:1}}>{it.label}</span>
              {it.count!=null && <MPBadge tone="neutral">{it.count}</MPBadge>}
              {it.kbd && <MPKbd>{it.kbd}</MPKbd>}
            </button>
          );
        })}
      </nav>
      <div style={{flex:1}}/>
      <div style={{padding:'8px', borderTop:'1px solid var(--mp-border)', marginTop:8}}>
        <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', display:'flex', justifyContent:'space-between'}}>
          <span>v0.1.0</span>
          <span style={{color:'var(--mp-success)'}}>● online</span>
        </div>
      </div>
    </div>
  );
}

function MPTopBar({ title, subtitle, right }) {
  return (
    <div style={{
      display:'flex', alignItems:'center', justifyContent:'space-between',
      padding:'14px 20px', borderBottom:'1px solid var(--mp-border)',
      flexShrink:0, background:'var(--mp-bg)',
    }}>
      <div>
        <div style={{fontSize:15, fontWeight:600, letterSpacing:'-0.01em'}}>{title}</div>
        {subtitle && <div style={{fontSize:12, color:'var(--mp-textMuted)', marginTop:2}}>{subtitle}</div>}
      </div>
      <div style={{display:'flex', gap:6, alignItems:'center'}}>{right}</div>
    </div>
  );
}

// ─────────── HOME ───────────
function ScreenHome({ onAction, onSelectJob }) {
  return (
    <>
      <MPTopBar title="Home" subtitle="Right-click files in Explorer, drag them here, or press ⌘N to start a job." right={
        <MPButton variant="primary" icon="plus" onClick={()=>onAction?.('newjob')}>New print job</MPButton>
      }/>
      <div className="mp-scroll" style={{flex:1, overflow:'auto', padding:20}}>
        {/* Drop zone */}
        <div style={{
          border:'1.5px dashed var(--mp-borderStrong)', borderRadius:10,
          padding:'28px 20px', textAlign:'center', background:'var(--mp-surface)',
          marginBottom: 20,
        }}>
          <div style={{display:'inline-flex', padding:10, background:'var(--mp-surface2)', borderRadius:10, marginBottom:10}}>
            <Icon name="upload" size={20} color="var(--mp-textMuted)"/>
          </div>
          <div style={{fontSize:14, fontWeight:500, marginBottom:4}}>Drop files or folders to print</div>
          <div style={{fontSize:12, color:'var(--mp-textMuted)'}}>
            PDF, Office, ODF, images, HTML, Markdown · no count limit
          </div>
        </div>

        {/* Quick actions */}
        <MPSectionLabel>Quick actions</MPSectionLabel>
        <div style={{display:'grid', gridTemplateColumns:'1fr 1fr 1fr', gap:10, marginBottom:24}}>
          {[
            { ico:'plus',   t:'New print job',     d:'Pick files and a profile',   k:'⌘N' },
            { ico:'folder', t:'Print a folder',    d:'Recurse or top-level only',  k:'⌘O' },
            { ico:'cmd',    t:'Run from CLI',      d:'maeclyprint print --files',  k:'⌘K' },
          ].map((q,i) => (
            <MPCard key={i} hoverable onClick={()=>onAction?.('newjob')} padding={14}>
              <div style={{display:'flex', alignItems:'center', justifyContent:'space-between', marginBottom:8}}>
                <Icon name={q.ico} size={16} color="var(--mp-accent)"/>
                <MPKbd>{q.k}</MPKbd>
              </div>
              <div style={{fontSize:13, fontWeight:500}}>{q.t}</div>
              <div style={{fontSize:11.5, color:'var(--mp-textMuted)', marginTop:2}}>{q.d}</div>
            </MPCard>
          ))}
        </div>

        {/* Recent activity */}
        <div style={{display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:8}}>
          <MPSectionLabel>Recent activity</MPSectionLabel>
          <button onClick={()=>onAction?.('queue')} className="mp-focus" style={{
            background:'none', border:'none', cursor:'pointer', fontSize:11.5, color:'var(--mp-textMuted)',
            display:'inline-flex', alignItems:'center', gap:3,
          }}>View all queue<Icon name="chevr" size={11}/></button>
        </div>
        <MPCard padding={0}>
          {MP_QUEUE_SAMPLE.slice(0,5).map((j,i,arr) => (
            <div key={j.id} onClick={()=>onSelectJob?.(j.id)} style={{
              display:'flex', alignItems:'center', gap:10, padding:'10px 14px',
              borderBottom: i<arr.length-1 ? '1px solid var(--mp-border)' : 'none',
              cursor:'pointer',
            }}>
              <FileGlyph ext={j.ext} size={22}/>
              <div style={{flex:1, minWidth:0}}>
                <div style={{fontSize:12.5, fontWeight:500, overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{j.file}</div>
                <div className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textDim)', marginTop:1}}>
                  {j.printer} · {j.profile}
                </div>
              </div>
              <StatusDot status={j.status}/>
              <span className="mp-mono" style={{fontSize:10.5, color:'var(--mp-textMuted)', textTransform:'uppercase', minWidth:60, textAlign:'right'}}>
                {j.status}
              </span>
            </div>
          ))}
        </MPCard>

        {/* Tip */}
        <div style={{marginTop:20, padding:'10px 12px', background:'var(--mp-infoSoft)', borderRadius:6, display:'flex', gap:8, alignItems:'flex-start'}}>
          <Icon name="info" size={14} color="var(--mp-info)"/>
          <div style={{fontSize:12, color:'var(--mp-info)', lineHeight:1.5}}>
            <strong>Tip · </strong>
            MaeclyPrint adds itself to Windows 11's right-click menu. Select any number of files, right-click, and choose <span className="mp-mono">Print with MaeclyPrint</span> — no 15-file limit.
          </div>
        </div>
      </div>
    </>
  );
}

Object.assign(window, { MPWinChrome, MPSidebar, MPTopBar, ScreenHome });
