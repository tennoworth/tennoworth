import { test, expect } from '@playwright/test';
for (const theme of ['light', 'dark'] as const) {
  test(`combined update notes remain readable and reopen from Settings in ${theme}`, async ({ page }, testInfo) => {
    const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));
    await page.emulateMedia({colorScheme:theme});
    await page.goto('/?preview-desktop&sample=update-notes');
    const dialog=page.getByRole('dialog',{name:'What’s new'});
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('3 releases included')).toBeVisible();
    await expect(dialog.getByText('v0.7.1 → v0.7.103')).toBeVisible();
    await dialog.getByRole('button',{name:'Show all changes (7)'}).click();
    await dialog.getByText('See changes by version (3)').click();
    for(const [width,height] of [[1440,1000],[760,700],[320,480],[1440,480]]) {
      await page.setViewportSize({width,height});
      const bounds=await dialog.boundingBox();
      expect(bounds!.x).toBeGreaterThanOrEqual(0);expect(bounds!.y).toBeGreaterThanOrEqual(0);
      expect(bounds!.x+bounds!.width).toBeLessThanOrEqual(width+1);expect(bounds!.y+bounds!.height).toBeLessThanOrEqual(height+1);
      expect(await dialog.evaluate(el=>el.scrollWidth<=el.clientWidth)).toBe(true);
      await dialog.getByRole('button',{name:'Got it'}).scrollIntoViewIfNeeded();
      await expect(dialog.getByRole('button',{name:'Got it'})).toBeInViewport();
      await dialog.evaluate(el=>el.scrollTop=0);
      await page.screenshot({path:testInfo.outputPath(`notes-${theme}-${width}-${height}.png`)});
    }
    await page.keyboard.press('Escape');await expect(dialog).not.toBeVisible();
    await page.locator('.sidebar').getByRole('button',{name:/^Settings/}).click();
    const reopen=page.getByRole('button',{name:'What’s new',exact:true});await reopen.click();
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('Installed version',{exact:true})).toBeVisible();
    await dialog.getByRole('button',{name:'Close what’s new'}).click();
    await expect(reopen).toBeFocused();
    await page.waitForTimeout(850);await expect(dialog).not.toBeVisible();
    expect(errors).toEqual([]);
  });
}
test('single release and unknown previous version use honest context',async ({page})=>{
  await page.goto('/?preview-desktop&sample=update-notes-single');
  const dialog=page.getByRole('dialog',{name:'What’s new'});
  await expect(dialog).toBeVisible();await expect(dialog.getByText('1 release included')).toBeVisible();
  await expect(dialog.getByText('v0.7.102 → v0.7.103')).toBeVisible();
  await page.goto('/?preview-desktop&sample=update-notes-unknown');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText(/earlier app version wasn’t recorded/)).toBeVisible();
  await expect(dialog.getByText('Update installed',{exact:true})).toHaveCount(0);
});
test('another dialog and loss of native focus defer automatic notes',async ({page})=>{
  await page.addInitScript(()=>{
    let runtime: any;
    let tauri: any;
    Object.defineProperty(window,'__TAURI__',{configurable:true,get:()=>tauri,set:value=>{
      const invoke=value.core.invoke;
      value.core.invoke=(cmd:string,args:unknown)=>cmd==='update_notes_can_present'?Promise.resolve((window as any).__notesFocused):invoke(cmd,args);
      tauri=value;
    }});
    (window as any).__notesFocused=false;
    Object.defineProperty(window,'__TAURI_INTERNALS__',{configurable:true,get:()=>runtime,set:value=>{
      const invoke=value.invoke;
      value.invoke=(cmd:string,args:unknown)=>cmd==='update_notes_can_present'?Promise.resolve((window as any).__notesFocused):invoke(cmd,args);
      runtime=value;
    }});
  });
  await page.goto('/?preview-desktop&sample=update-notes');
  const dialog=page.getByRole('dialog',{name:'What’s new'});
  await page.waitForTimeout(1000);await expect(dialog).not.toBeVisible();
  await page.evaluate(()=>{
    const blocker=document.createElement('dialog');blocker.id='blocking-dialog';blocker.textContent='Another task';document.body.append(blocker);blocker.showModal();
    (window as any).__notesFocused=true;
  });
  await page.waitForTimeout(1000);await expect(dialog).not.toBeVisible();
  await page.evaluate(()=>document.querySelector<HTMLDialogElement>('#blocking-dialog')!.close());
  await expect(dialog).toBeVisible();
  for(let i=0;i<12;i++){await page.keyboard.press('Tab');expect(await dialog.evaluate(el=>el.contains(document.activeElement))).toBe(true);}
});
