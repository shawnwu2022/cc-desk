interface ProjectToggle {
  getAttribute(name: string): Promise<string | null>
  click(): Promise<void>
}
interface ProjectTreePage {
  locator(selector: string): { all(): Promise<ProjectToggle[]> }
}

/** Shared by the browser setup and its real-fixture DOM regression. */
export async function expandFixtureProjects(page: ProjectTreePage) {
  // all() returns live nth locators, so membership must remain stable after a click.
  for (const row of await page.locator('.project-node > .project-row .expand-arrow').all()) {
    if (await row.getAttribute('aria-expanded') === 'false') await row.click()
  }
}

interface SessionMenuPage {
  locator(selector: string): { first(): {
    focus(): Promise<void>
    locator(selector: string): { click(): Promise<void> }
  } }
}

export async function openFixtureSessionMenu(page: SessionMenuPage) {
  const row = page.locator('[data-session-row]').first()
  // Production enables overflow pointer events through the row's focus-within state.
  await row.focus()
  await row.locator('.session-overflow-trigger button').click()
}
