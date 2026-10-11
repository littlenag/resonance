export interface Item {
  name: string;
  quantity: number;
}

// Shows the total quantity of all items.
export function totalLabel(items: Item[]): string {
  const total = items.reduce((sum, item) => sum + item.quantity, 0);
  return `Total: ${total}`;
}
