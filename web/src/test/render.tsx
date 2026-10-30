import type { ReactNode } from 'react';
import { render } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { Providers, makeQueryClient } from '../Providers';

export function renderAt(path: string, routes: { path: string; element: ReactNode }[]) {
  return render(
    <Providers client={makeQueryClient()}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          {routes.map((r) => (
            <Route key={r.path} path={r.path} element={r.element} />
          ))}
        </Routes>
      </MemoryRouter>
    </Providers>,
  );
}
