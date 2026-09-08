import { render } from '@testing-library/svelte';
import { createDesktopServices } from '../adapters/services';
import { DESKTOP_CONTEXT } from '../contracts/services';
export const renderDesktop: typeof render = (component, options, renderOptions) => render(component, Object.assign({}, options, { context: new Map([[DESKTOP_CONTEXT, createDesktopServices()]]) }) as typeof options, renderOptions);
