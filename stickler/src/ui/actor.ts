import { Page } from 'playwright';
import { Action, UIState } from './types.js';

interface ActorConfig {
  actionDelay: {
    min: number;
    max: number;
  };
}

/**
 * Execute an action on the page with human-like behavior
 */
export async function executeAction(
  page: Page,
  action: Action,
  uiState: UIState,
  config: ActorConfig
): Promise<{ success: boolean; error?: string }> {
  try {
    // Add random delay before action (human-like timing)
    await randomDelay(config.actionDelay.min, config.actionDelay.max);

    switch (action.type) {
      case 'click':
        await executeClick(page, action.target, uiState);
        break;

      case 'type':
        await executeType(page, action.target, action.text, uiState);
        break;

      case 'scroll':
        await executeScroll(page, action.direction, action.amount);
        break;

      case 'wait':
        await page.waitForTimeout(action.duration);
        break;

      case 'done':
      case 'stuck':
        // No action to execute, just return success
        return { success: true };

      default:
        return { success: false, error: `Unknown action type: ${(action as any).type}` };
    }

    // Wait for page to stabilize after action
    await page.waitForLoadState('networkidle', { timeout: 1000 }).catch(() => {
      // Ignore timeout - page might be continuously loading
    });

    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

/**
 * Click an element using human-like mouse movement
 */
async function executeClick(page: Page, targetLabel: string, uiState: UIState): Promise<void> {
  // Find the element in our captured state
  const element = uiState.interactables.find((el) => el.label === targetLabel);

  if (!element) {
    throw new Error(`Element not found: "${targetLabel}"`);
  }

  const { boundingBox } = element;

  // Calculate click position (center with small random offset)
  const x = boundingBox.x + boundingBox.width / 2 + randomOffset(5);
  const y = boundingBox.y + boundingBox.height / 2 + randomOffset(5);

  // Move mouse to position with human-like movement
  await page.mouse.move(x, y, { steps: randomInt(10, 20) });

  // Small delay before click
  await randomDelay(50, 150);

  // Click
  await page.mouse.click(x, y);

  console.log(`   Clicked "${targetLabel}" at (${Math.round(x)}, ${Math.round(y)})`);
}

/**
 * Type text into an input with human-like delays
 */
async function executeType(page: Page, targetLabel: string, text: string, uiState: UIState): Promise<void> {
  // Find the element
  const element = uiState.interactables.find((el) => el.label === targetLabel);

  if (!element) {
    throw new Error(`Input element not found: "${targetLabel}"`);
  }

  const { boundingBox } = element;

  // Click to focus the input first
  const x = boundingBox.x + boundingBox.width / 2 + randomOffset(5);
  const y = boundingBox.y + boundingBox.height / 2 + randomOffset(5);

  await page.mouse.move(x, y, { steps: randomInt(10, 20) });
  await randomDelay(50, 150);
  await page.mouse.click(x, y);

  // Wait for focus
  await randomDelay(100, 200);

  // Clear existing value if any
  if (element.value) {
    await page.keyboard.press('Control+A');
    await randomDelay(50, 100);
  }

  // Type character by character with human-like delays
  for (const char of text) {
    await page.keyboard.type(char, { delay: randomInt(50, 150) });
  }

  console.log(`   Typed "${text}" into "${targetLabel}"`);
}

/**
 * Scroll the page
 */
async function executeScroll(page: Page, direction: 'up' | 'down', amount: number = 300): Promise<void> {
  const delta = direction === 'down' ? amount : -amount;

  // Smooth scroll with multiple smaller steps
  const steps = 5;
  const stepSize = delta / steps;

  for (let i = 0; i < steps; i++) {
    await page.mouse.wheel(0, stepSize);
    await randomDelay(50, 100);
  }

  console.log(`   Scrolled ${direction} by ${amount}px`);
}

/**
 * Random delay between min and max milliseconds
 */
function randomDelay(min: number, max: number): Promise<void> {
  const delay = randomInt(min, max);
  return new Promise((resolve) => setTimeout(resolve, delay));
}

/**
 * Random integer between min and max (inclusive)
 */
function randomInt(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

/**
 * Small random offset for more natural clicking
 */
function randomOffset(maxOffset: number): number {
  return (Math.random() - 0.5) * maxOffset * 2;
}
