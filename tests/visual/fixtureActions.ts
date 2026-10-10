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

/** Clear setup focus through real DOM events without leaving a new focus target. */
export async function clearFixtureSetupFocus(page: FixtureFocusPage) {
  await page.locator('.shell-main').evaluate(element => {
    if (element instanceof HTMLElement) {
      const previousTabIndex = element.getAttribute('tabindex')
      element.tabIndex = -1
      element.focus()
      element.blur()
      if (previousTabIndex === null) element.removeAttribute('tabindex')
      else element.setAttribute('tabindex', previousTabIndex)
    }
  })
}

/** Shared by the browser setup and its real-fixture DOM regression. */
export async function expandFixtureProjects(page: ProjectTreePage) {
  // all() returns live nth locators, so membership must remain stable after a click.
  for (const row of await page.locator('.project-node > .project-row .expand-arrow').all()) {
    if (await row.getAttribute('aria-expanded') === 'false') await row.click()
  }
  await clearFixtureSetupFocus(page)
}

interface SessionMenuPage {
  locator(selector: string): { first(): {
    focus(): Promise<void>
    locator(selector: string): { click(): Promise<void> }
  } }
}

export async function openFixtureSessionMenu(page: SessionMenuPage) {
  const row = page.locator('[data-session-row="visual-session-0"]').first()
  // Production enables overflow pointer events through the row's focus-within state.
  await row.focus()
  await row.locator('.session-overflow-trigger button').click()
}
