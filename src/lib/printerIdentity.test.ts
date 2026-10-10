import { describe, expect, it } from 'vitest';
import { resolvePrinterId } from './printerIdentity';

const printers = [
  { id: 'HP_LaserJet_Pro_M404', name: 'HP LaserJet Pro M404' },
  { id: 'Maecly_Hold', name: 'Pluryprint hold test queue' },
  { id: 'A', name: 'Shared' },
  { id: 'B', name: 'Shared' },
];

describe('resolvePrinterId', () => {
  it('FR-044: resolves an id or a display name to the queue id', () => {
    for (const ref of [
      'HP_LaserJet_Pro_M404',
      'hp_laserjet_pro_m404',
      'HP LaserJet Pro M404',
      ' hp laserjet pro m404 ',
    ]) {
      expect(resolvePrinterId(printers, ref)).toBe('HP_LaserJet_Pro_M404');
    }
  });

  it('FR-044: refuses unknown, empty and ambiguous references', () => {
    expect(resolvePrinterId(printers, 'Nope')).toBeNull();
    expect(resolvePrinterId(printers, '')).toBeNull();
    expect(resolvePrinterId(printers, undefined)).toBeNull();
    expect(resolvePrinterId(printers, 'Shared')).toBeNull();
  });
});
