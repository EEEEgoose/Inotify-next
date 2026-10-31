<template>
  <div class="stack">
    <div class="page-head">
      <h2>发送统计</h2>
      <el-radio-group v-model="days" size="small" @change="load">
        <el-radio-button :value="7">7 天</el-radio-button>
        <el-radio-button :value="30">30 天</el-radio-button>
        <el-radio-button :value="90">90 天</el-radio-button>
      </el-radio-group>
    </div>

    <div v-if="stats" class="tiles">
      <el-card shadow="never"><el-statistic title="成功" :value="total.success" /></el-card>
      <el-card shadow="never"><el-statistic title="失败" :value="total.failed" /></el-card>
      <el-card shadow="never"><el-statistic title="用户" :value="stats.users" /></el-card>
      <el-card shadow="never"><el-statistic title="渠道" :value="stats.channels" /></el-card>
    </div>

    <el-card shadow="never">
      <template #header>每日发送量</template>
      <div ref="trend" class="chart" />
    </el-card>

    <el-card shadow="never">
      <template #header>按渠道类型</template>
      <el-table :data="stats?.kinds ?? []" empty-text="暂无数据">
        <el-table-column label="类型" prop="name" />
        <el-table-column label="成功" prop="success" align="right" />
        <el-table-column label="失败" prop="failed" align="right" />
        <el-table-column label="成功率" align="right">
          <template #default="{ row }">{{ rate(row.success, row.failed) }}</template>
        </el-table-column>
      </el-table>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import * as echarts from 'echarts/core'
import { BarChart } from 'echarts/charts'
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { api, type Stats } from '../../api'

echarts.use([BarChart, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer])

const days = ref(30)
const stats = ref<Stats | null>(null)
const trend = ref<HTMLDivElement>()
let chart: echarts.ECharts | null = null

const total = computed(() =>
  (stats.value?.kinds ?? []).reduce(
    (t, k) => ({ success: t.success + k.success, failed: t.failed + k.failed }),
    { success: 0, failed: 0 },
  ),
)
const rate = (success: number, failed: number) =>
  success + failed === 0 ? '—' : `${((success / (success + failed)) * 100).toFixed(1)}%`

/** Every date in the range, so days without sends show as zero. */
function dateRange(n: number): string[] {
  const out: string[] = []
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date()
    d.setDate(d.getDate() - i)
    out.push(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`)
  }
  return out
}

function render() {
  if (!trend.value || !stats.value) return
  chart ??= echarts.init(trend.value)
  const dates = dateRange(days.value)
  const byDate = new Map(stats.value.days.map((d) => [d.date, d]))
  const style = getComputedStyle(document.documentElement)
  const text = style.getPropertyValue('--el-text-color-secondary').trim()
  const line = style.getPropertyValue('--el-border-color-lighter').trim()
  chart.setOption({
    tooltip: { trigger: 'axis' },
    legend: { data: ['成功', '失败'], top: 0, right: 0, textStyle: { color: text } },
    grid: { left: 40, right: 16, top: 36, bottom: 28 },
    xAxis: { type: 'category', data: dates.map((d) => d.slice(5)), axisLabel: { color: text } },
    yAxis: { type: 'value', minInterval: 1, axisLabel: { color: text }, splitLine: { lineStyle: { color: line } } },
    series: [
      {
        name: '成功',
        type: 'bar',
        stack: 'all',
        data: dates.map((d) => byDate.get(d)?.success ?? 0),
        itemStyle: { color: style.getPropertyValue('--el-color-success').trim() },
      },
      {
        name: '失败',
        type: 'bar',
        stack: 'all',
        data: dates.map((d) => byDate.get(d)?.failed ?? 0),
        itemStyle: { color: style.getPropertyValue('--el-color-danger').trim() },
      },
    ],
  })
}

async function load() {
  try {
    stats.value = await api.stats(days.value)
    await nextTick()
    render()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

const resize = () => chart?.resize()
onMounted(() => {
  load()
  window.addEventListener('resize', resize)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', resize)
  chart?.dispose()
})
</script>

<style scoped>
.tiles {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 16px;
}
.chart {
  height: 280px;
}
</style>
