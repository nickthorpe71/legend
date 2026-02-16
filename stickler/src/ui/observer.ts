import { Page } from 'playwright';
import { UIState, InteractableElement, BoundingBox } from './types.js';

export async function captureUIState(page: Page): Promise<UIState> {
  // Wait for any pending navigation to complete
  await page.waitForLoadState('domcontentloaded', { timeout: 3000 }).catch(() => {});

  // Wait a bit for any client-side routing to settle
  await page.waitForTimeout(500);

  const screenshot = await page.screenshot({ fullPage: false });
  const url = page.url();
  const interactables = await extractInteractables(page);
  const visibleText = await extractVisibleText(page);

  return {
    url,
    screenshot,
    interactables,
    visibleText,
    timestamp: Date.now(),
  };
}

async function extractInteractables(page: Page): Promise<InteractableElement[]> {
  try {
    return await page.evaluate(`
    (function() {
      const elements = [];

      function getBoundingBox(el) {
        const rect = el.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return null;
        if (rect.top > window.innerHeight || rect.bottom < 0) return null;
        if (rect.left > window.innerWidth || rect.right < 0) return null;
        return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
      }

      function getLabel(el) {
        const ariaLabel = el.getAttribute('aria-label');
        if (ariaLabel) return ariaLabel.trim();
        const textContent = el.textContent && el.textContent.trim();
        if (textContent && textContent.length < 100) return textContent;
        if (el instanceof HTMLInputElement && el.placeholder) return el.placeholder.trim();
        if (el instanceof HTMLInputElement && el.value) return el.value.trim();
        const name = el.getAttribute('name');
        if (name) return name;
        const id = el.getAttribute('id');
        if (id) return id;
        return 'unlabeled';
      }

      document.querySelectorAll('button').forEach(function(button) {
        const box = getBoundingBox(button);
        if (box) elements.push({ type: 'button', label: getLabel(button), role: button.getAttribute('role'), boundingBox: box });
      });

      document.querySelectorAll('a[href]').forEach(function(link) {
        const box = getBoundingBox(link);
        if (box) elements.push({ type: 'link', label: getLabel(link), role: link.getAttribute('role'), boundingBox: box });
      });

      document.querySelectorAll('input:not([type="hidden"])').forEach(function(input) {
        const box = getBoundingBox(input);
        if (box) elements.push({ type: 'input', label: getLabel(input), role: input.getAttribute('role'), boundingBox: box, placeholder: input.placeholder, value: input.value });
      });

      document.querySelectorAll('textarea').forEach(function(textarea) {
        const box = getBoundingBox(textarea);
        if (box) elements.push({ type: 'input', label: getLabel(textarea), role: textarea.getAttribute('role'), boundingBox: box, placeholder: textarea.placeholder, value: textarea.value });
      });

      document.querySelectorAll('select').forEach(function(select) {
        const box = getBoundingBox(select);
        if (box) elements.push({ type: 'select', label: getLabel(select), role: select.getAttribute('role'), boundingBox: box });
      });

      document.querySelectorAll('input[type="checkbox"]').forEach(function(checkbox) {
        const box = getBoundingBox(checkbox);
        if (box) elements.push({ type: 'checkbox', label: getLabel(checkbox), role: checkbox.getAttribute('role'), boundingBox: box });
      });

      document.querySelectorAll('[role="button"]').forEach(function(el) {
        if (el.tagName !== 'BUTTON') {
          const box = getBoundingBox(el);
          if (box) elements.push({ type: 'other', label: getLabel(el), role: el.getAttribute('role'), boundingBox: box });
        }
      });

      return elements;
    })()
  `);
  } catch (error) {
    console.warn('Failed to extract interactables (page may have navigated):', error instanceof Error ? error.message : String(error));
    return [];
  }
}

async function extractVisibleText(page: Page): Promise<string[]> {
  try {
    return await page.evaluate(`
    (function() {
      const texts = [];

      function isVisible(el) {
        const rect = el.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0 && rect.top < window.innerHeight;
      }

      ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'].forEach(function(tag) {
        document.querySelectorAll(tag).forEach(function(heading) {
          if (isVisible(heading)) {
            const text = heading.textContent && heading.textContent.trim();
            if (text) texts.push('[' + tag.toUpperCase() + '] ' + text);
          }
        });
      });

      document.querySelectorAll('p, div, span').forEach(function(el) {
        if (isVisible(el)) {
          const text = el.textContent && el.textContent.trim();
          if (text) {
            const isErrorLike = /error|warning|invalid|fail|incorrect|required|missing/i.test(text);
            const isShortMessage = text.length < 150;
            if (isErrorLike || isShortMessage) {
              if (!texts.some(function(t) { return t.includes(text); })) {
                texts.push(text);
              }
            }
          }
        }
      });

      document.querySelectorAll('label').forEach(function(label) {
        if (isVisible(label)) {
          const text = label.textContent && label.textContent.trim();
          if (text && text.length < 100) {
            texts.push('[LABEL] ' + text);
          }
        }
      });

      return texts;
    })()
  `);
  } catch (error) {
    console.warn('Failed to extract visible text (page may have navigated):', error instanceof Error ? error.message : String(error));
    return [];
  }
}
