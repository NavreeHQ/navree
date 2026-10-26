from django.urls import path

from crm import views

urlpatterns = [
    path("json", views.JsonView.as_view()),
    path("db", views.SingleQueryView.as_view()),
    path("queries", views.MultipleQueriesView.as_view()),
    path("api/deals", views.DealListCreateView.as_view()),
]
