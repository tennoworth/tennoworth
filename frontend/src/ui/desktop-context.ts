import { getContext } from 'svelte';
import { DESKTOP_CONTEXT, type DesktopServices } from '../contracts/services';
export function useDesktopServices(): DesktopServices {
  const services = getContext<DesktopServices>(DESKTOP_CONTEXT);
  if (!services) throw new Error('Desktop services were not provided by this surface.');
  return services;
}
