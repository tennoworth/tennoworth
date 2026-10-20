export const pages = [
  { path: '/guides/sell-prime-sets-or-parts/', kind: 'sets', title: 'Warframe: Sell Prime Sets or Parts?', heading: 'Sell Prime sets or parts?', description: 'Compare Warframe Prime sets with their component prices, quantities and market activity before deciding what to sell.' },
  { path: '/tools/ducats-vs-platinum/', kind: 'ducats', title: 'Warframe Ducats vs Platinum Calculator', heading: 'Ducats or platinum?', description: 'Compare the platinum opportunity cost of turning Warframe Prime parts into ducats, with dated market prices and clear assumptions.' },
  { path: '/desktop/', kind: 'desktop', title: 'Warframe Inventory Tool for Windows and Linux', heading: 'Your inventory. A clearer sell list.', description: 'A free, open-source Warframe inventory and selling tool for Windows and Linux. No Overwolf; inventory stays local.' },
] as const;
export const pilotSets = ['bo_prime_set', 'braton_prime_set', 'burston_prime_set'];
export const itemPath = (slug: string) => `/items/${slug}/`;
export const pilotItems = ['bo_prime_set', 'bo_prime_blueprint', 'bo_prime_handle', 'bo_prime_ornament', 'braton_prime_set', 'braton_prime_barrel', 'braton_prime_blueprint', 'braton_prime_receiver', 'braton_prime_stock', 'burston_prime_set', 'burston_prime_barrel', 'burston_prime_blueprint', 'burston_prime_receiver', 'burston_prime_stock'];
