import { expect, it } from 'vitest';
import { isMarket, replacesMarket } from './market';
import cases from '../../../tests/fixtures/market-contract/cases.json';
import ordering from '../../../tests/fixtures/market-contract/ordering.json';
import published from '../../public/market.json';

it.each(cases)('market contract: $name', ({ body, valid }) => {
  expect(isMarket(body)).toBe(valid);
});

it.each(ordering)('snapshot ordering: $name', ({ current, next, now, replaces }) => {
  expect(replacesMarket(current, next, Date.parse(now))).toBe(replaces);
});

it('the bundled snapshot satisfies the runtime contract', () => {
  expect(isMarket(published)).toBe(true);
});
