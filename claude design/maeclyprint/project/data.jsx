// Fake data + state for MaeclyPrint prototype

const MP_PRINTERS = [
  { id:'hp404',  name:'HP LaserJet Pro M404dn',  type:'laser',  color:false, duplex:true,  trays:['Tray 1','Tray 2','Manual'] },
  { id:'sharp',  name:'Sharp MX-3071',           type:'mfp',    color:true,  duplex:true,  trays:['Tray 1','Tray 2','Tray 3','Bypass'] },
  { id:'brother',name:'Brother HL-L8360CDW',     type:'laser',  color:true,  duplex:true,  trays:['Tray 1','Manual'] },
  { id:'roland', name:'Roland VersaSTUDIO BN-20',type:'wide',   color:true,  duplex:false, trays:['Roll'] },
  { id:'zebra',  name:'Zebra ZD420',             type:'label',  color:false, duplex:false, trays:['Roll'] },
  { id:'pdf',    name:'Microsoft Print to PDF',  type:'virtual',color:true,  duplex:false, trays:['—'] },
];

const MP_PROFILES = [
  { id:'p1', name:'Quick',         builtin:true,  default:true,
    printer:'<default>', color:'color',      duplex:'off',       copies:1, paper:'A4',     orientation:'auto', scaling:'fit',  quality:'normal', tray:'auto' },
  { id:'p2', name:'Duplex BW',     builtin:true,
    printer:'<default>', color:'monochrome', duplex:'long-edge', copies:1, paper:'A4',     orientation:'auto', scaling:'fit',  quality:'normal', tray:'auto' },
  { id:'p3', name:'Photo Color',   builtin:true,
    printer:'<default>', color:'color',      duplex:'off',       copies:1, paper:'A4',     orientation:'auto', scaling:'none', quality:'high',   tray:'auto' },
  { id:'p4', name:'Press A · Duplex Long', builtin:false,
    printer:'Sharp MX-3071', color:'color',  duplex:'long-edge', copies:1, paper:'A3',     orientation:'auto', scaling:'fit',  quality:'high',   tray:'Tray 2' },
  { id:'p5', name:'Letterhead Stationery',  builtin:false,
    printer:'HP LaserJet Pro M404dn', color:'monochrome', duplex:'off', copies:1, paper:'Letter', orientation:'portrait', scaling:'fit', quality:'high', tray:'Tray 2' },
  { id:'p6', name:'Zebra Labels',  builtin:false,
    printer:'Zebra ZD420', color:'monochrome', duplex:'off', copies:1, paper:'Letter', orientation:'portrait', scaling:'none', quality:'normal', tray:'Roll' },
];

function mkFile(name, ext, kb, status='queued', pages, progress) {
  return { id: name+'_'+ext, name, ext, kb, status, pages: pages ?? null, progress: progress ?? 0 };
}

const MP_FILES_SAMPLE = [
  mkFile('Q3-Financial-Report',     'pdf',  2840, 'queued', 24),
  mkFile('Contract-AcmeCorp-v3',    'pdf',  1120, 'queued', 8),
  mkFile('Exhibit-A',               'pdf',   980, 'queued', 12),
  mkFile('Exhibit-B',               'pdf',   612, 'queued', 4),
  mkFile('Cover-Letter',            'docx',   42, 'queued', 1),
  mkFile('Schedule-Q4',             'xlsx',   88, 'queued', 3),
  mkFile('Site-Photo-North',        'jpg',  4320, 'queued', 1),
  mkFile('Site-Photo-South',        'jpg',  3950, 'queued', 1),
  mkFile('Floorplan-Ground',        'pdf',  1850, 'queued', 2),
  mkFile('Lab-Handout-Week-3',      'pdf',   720, 'queued', 6),
];

const MP_QUEUE_SAMPLE = [
  { id:'j1', file:'Q3-Financial-Report.pdf',     ext:'pdf',  status:'printing',   progress:0.62, pages:24, profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'10:42:01', sizeKb:2840 },
  { id:'j2', file:'Contract-AcmeCorp-v3.pdf',    ext:'pdf',  status:'converting', progress:0.34, pages:8,  profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'10:42:08', sizeKb:1120 },
  { id:'j3', file:'Exhibit-A.pdf',               ext:'pdf',  status:'queued',     progress:0,    pages:12, profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'—',         sizeKb:980  },
  { id:'j4', file:'Exhibit-B.pdf',               ext:'pdf',  status:'queued',     progress:0,    pages:4,  profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'—',         sizeKb:612  },
  { id:'j5', file:'Cover-Letter.docx',           ext:'docx', status:'queued',     progress:0,    pages:1,  profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'—',         sizeKb:42   },
  { id:'j6', file:'Schedule-Q4.xlsx',            ext:'xlsx', status:'paused',     progress:0,    pages:3,  profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'—',         sizeKb:88,   note:'Printer offline · retry in 5s' },
  { id:'j7', file:'Site-Photo-North.jpg',        ext:'jpg',  status:'done',       progress:1,    pages:1,  profile:'Photo Color', printer:'Sharp MX-3071',           startedAt:'10:39:14', finishedAt:'10:39:31', sizeKb:4320 },
  { id:'j8', file:'Site-Photo-South.jpg',        ext:'jpg',  status:'done',       progress:1,    pages:1,  profile:'Photo Color', printer:'Sharp MX-3071',           startedAt:'10:39:32', finishedAt:'10:39:48', sizeKb:3950 },
  { id:'j9', file:'Floorplan-Ground.pdf',        ext:'pdf',  status:'failed',     progress:0,    pages:2,  profile:'Quick',       printer:'Sharp MX-3071',           startedAt:'10:38:55', sizeKb:1850, error:'Paper jam in Tray 2' },
  { id:'j10',file:'Lab-Handout-Week-3.pdf',      ext:'pdf',  status:'done',       progress:1,    pages:6,  profile:'Duplex BW',   printer:'HP LaserJet Pro M404dn', startedAt:'10:35:20', finishedAt:'10:35:51', sizeKb:720 },
];

function fmtKb(kb) {
  if (kb < 1024) return kb + ' KB';
  return (kb/1024).toFixed(1) + ' MB';
}

Object.assign(window, { MP_PRINTERS, MP_PROFILES, MP_FILES_SAMPLE, MP_QUEUE_SAMPLE, mkFile, fmtKb });
