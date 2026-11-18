import ActivityKit
import SwiftUI
import WidgetKit

@main
struct NotchWidgetBundle: WidgetBundle {
    var body: some Widget { NotchLiveActivity() }
}

struct NotchLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: NotchActivityAttributes.self) { context in
            let s = context.state
            LiveCard(kind: s.kind, title: s.title, detail: s.detail, progress: s.progress, pc: context.attributes.pc)
                .activityBackgroundTint(Color(red: 0.05, green: 0.05, blue: 0.07))
                .activitySystemActionForegroundColor(Color.white)
        } dynamicIsland: { context in
            let s = context.state
            return DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Image(systemName: LiveStyle.icon(s.kind)).foregroundStyle(LiveStyle.tint(s.kind))
                }
                DynamicIslandExpandedRegion(.trailing) {
                    Text(LiveStyle.trailing(kind: s.kind, progress: s.progress)).monospacedDigit()
                }
                DynamicIslandExpandedRegion(.center) {
                    Text(s.title).lineLimit(1)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    if let p = s.progress { LiveBar(value: p, color: LiveStyle.tint(s.kind)) }
                }
            } compactLeading: {
                Image(systemName: LiveStyle.icon(s.kind)).foregroundStyle(LiveStyle.tint(s.kind))
            } compactTrailing: {
                Text(LiveStyle.trailing(kind: s.kind, progress: s.progress)).monospacedDigit()
            } minimal: {
                Image(systemName: LiveStyle.icon(s.kind)).foregroundStyle(LiveStyle.tint(s.kind))
            }
        }
    }
}
