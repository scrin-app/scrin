import { describe, expect, it } from 'vitest';

import { devicesSearch, homeSearch, sessionSearch } from './search';

describe('search validators', () => {
  it('accepts only 9-digit IDs', () => {
    expect(homeSearch({ id: '123456789' })).toEqual({ id: '123456789' });
    expect(homeSearch({ id: 123456789 })).toEqual({ id: '123456789' });
    expect(homeSearch({ id: '12345' })).toEqual({});
    expect(homeSearch({ id: '<script>' })).toEqual({});
  });

  it('bounds the session id', () => {
    expect(sessionSearch({ s: 's1-123' })).toEqual({ s: 's1-123' });
    expect(sessionSearch({ s: 'x'.repeat(65) })).toEqual({});
  });

  it('keeps only known devices params', () => {
    expect(devicesSearch({ q: 'lab', view: 'list', group: 'Office', extra: 1 })).toEqual({
      q: 'lab',
      view: 'list',
      group: 'Office',
    });
    expect(devicesSearch({ view: 'table' })).toEqual({});
  });
});
