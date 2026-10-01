interface ProjectToggle {
  getAttribute(name: string): Promise<string | null>
  click(): Promise<void>
}
interface FixtureFocusPage {
  locator(selector: string): { evaluate(action: (element: Element) => void): Promise<void> }
}
interface ProjectTreePage extends FixtureFocusPage {
  locator(selector: string): { all(): Promise<ProjectToggle[]>; evaluate(action: (element: Element) => void): Promise<void> }
}

/** Move focus through the DOM so setup controls do not leave an unrelated tooltip. */
export async function focusFixtureMain(page: FixtureFocusPage) {
  await page.locator('.shell-main').evaluate(element => {
    if (element instanceof HTMLElement) {
      element.tabIndex = -1
      element.focus()
    }
  })
}

/** Shared by the browser setup and its real-fixture DOM regression. */
export async function expandFixtureProjects(page: ProjectTreePage) {
  // all() returns live nth locators, so membership must remain stable after a click.
  for (const row of await page.locator('.project-node > .project-row .expand-arrow').all()) {
    if (await row.getAttribute('aria-expanded') === 'false') await row.click()
  }
  await focusFixtureMain(page)
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
