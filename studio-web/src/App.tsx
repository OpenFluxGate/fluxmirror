import { BrowserRouter, Route, Routes } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Layout } from './components/Layout'
import { File } from './routes/File'
import { Home } from './routes/Home'
import { NotFound } from './routes/NotFound'
import { Projects } from './routes/Projects'
import { Replay } from './routes/Replay'
import { SessionDetail } from './routes/Session'
import { Sessions } from './routes/Sessions'
import { Today } from './routes/Today'
import { Week } from './routes/Week'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 5_000,
    },
  },
})

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route element={<Layout />}>
            <Route index element={<Home />} />
            <Route path="today" element={<Today />} />
            <Route path="week" element={<Week />} />
            <Route path="sessions" element={<Sessions />} />
            <Route path="session/:id" element={<SessionDetail />} />
            <Route path="projects" element={<Projects />} />
            <Route path="file/*" element={<File />} />
            <Route path="replay" element={<Replay />} />
            <Route path="replay/:date" element={<Replay />} />
            <Route path="*" element={<NotFound />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  )
}
